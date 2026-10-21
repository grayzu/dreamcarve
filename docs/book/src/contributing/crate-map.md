# Crate map

DreamCarve is a Cargo workspace. Library crates carry a `dc-` prefix; the
command line binary is `dreamcarve`. Library crates never panic, print, or
do I/O: they take values and return `Result`s and warnings. The binary does
all file, terminal, and exit-code handling.

## How the pieces fit

```text
coaster.svg ──dc-import──▶ definitions + instances ─┐
                                                    ├─▶ Project (dc-project) ──save/load──▶ coaster.dcv
          init defaults (dreamcarve) ───────────────┘
coaster.dcv ──load + validate (dc-project)──▶ Project
            ──generate_profile (dc-cam)──▶ Toolpath (project space, mm, Y-up)
            ──emit (dc-gcode)──▶ Program text ──parse + safety check (dc-gcode)──▶ coaster.nc
```

Dependencies point one way. `dc-gcode` depends on `dc-cam` only for the
`Toolpath` type it consumes.

| Crate | Owns | Depends on |
| --- | --- | --- |
| `dreamcarve` | The command line. All file, terminal, clock and exit-code handling. | every `dc-` crate |
| `dc-library` | The tool library: its file format, load and save, adding, finding and removing tools. Bytes in, values out; the binary owns `tools.json`. | `dc-project` |
| `dc-gcode` | G-code emission and the safety pass that parses every emitted program. | `dc-cam`, `dc-project`, `dc-geometry` |
| `dc-cam` | Toolpath generators. Pure functions from project and operation to toolpath plus warnings. | `dc-project`, `dc-geometry` |
| `dc-import` | Importers. Flips imported Y into the internal Y-up frame. | `dc-project`, `dc-geometry` |
| `dc-project` | The project data model, the bundled post-processor definitions, `.dcv` load and save, migrations, validation, undo history. | `dc-geometry` |
| `dc-geometry` | Points, polylines with bulge arcs, transforms, offsets, flattening, nesting. Knows nothing about tools or stock. | nothing internal |

## Modules

### `dc-geometry` (`crates/dc-geometry/src/`)

| Module | Contents |
| --- | --- |
| `point.rs` | `Point`, `Point3`, `Vec2`, `Rect`; `EPS_MM` and `approx_eq`, used instead of `==` on floats; `MAX_COORDINATE_MM`, the 6 m bound the importer and validation share. |
| `polyline.rs` | `Polyline`, `Vertex`, `Segment`, `Arc`: lines and DXF-style bulge arcs; length, signed area, bounds, point at distance. |
| `contour.rs` | `Contour`: a closed polyline with known orientation (outer CCW, hole CW). |
| `transform.rs` | `Transform`: 2D affine, serialized as `[a, b, c, d, e, f]`. |
| `flatten.rs` | Bézier paths to polylines at a tolerance (kurbo); `FLATTEN_TOLERANCE_DEFAULT_MM`. |
| `offset.rs` | Parallel offset with round joins (cavalier_contours), arcs kept. |
| `region.rs` | `Region` and `nest`: loose contours to parts with holes. See [Shapes, holes, and islands](../user-guide/shapes-and-holes.md). |

### `dc-project` (`crates/dc-project/src/`)

| Module | Contents |
| --- | --- |
| `model.rs` | `Project` and everything in it: `Stock`, `MachineOrigin`, `FeatureDefinition`, `FeatureInstance`, `Tool`, `Operation`/`ProfileOp`, `TabSpec`. |
| `post_processor.rs` | `PostProcessor`, the controller dialect; `PostProcessorRef`, what a project stores (a bundled definition's id and version, and the user's settings); and the bundled definitions, by id and version. |
| `definitions/` | `<id>/v<n>.json`, one file per definition version, compiled in and never edited once released, and a README with their verification against controller sources. |
| `ids.rs` | `DefinitionId`, `InstanceId`, `ToolId`, `OperationId` newtypes. |
| `serialize.rs` | `save`, `load` (strict: unknown and missing fields fail), `Loaded`, `LoadError`. |
| `migrate.rs` | `CURRENT_VERSION` (7) and the raw-JSON migration steps from older versions. |
| `validate.rs` | `validate`, `Problem`, `Severity`: every problem at once, errors and warnings. |
| `history.rs` | `History`: snapshot undo and redo. Nothing drives it in phase 1. |
| `test_support.rs` | Fixture builders such as `sample_project()`; other crates' tests use them through the `test-support` feature. |

### `dc-import` (`crates/dc-import/src/`)

| Module | Contents |
| --- | --- |
| `svg.rs` | `import_svg`: usvg parses; each path becomes one definition and one instance, flipped to Y-up; circular arcs become bulges, other curves are flattened. |
| `svg/arc.rs` | Recognising usvg's Bézier pieces of circular arcs (circles, equal-radius ellipses and `A` commands, rounded-rect corners) exactly, and joining them back into arcs (#70). |
| `dxf.rs` | `import_dxf`: the `dxf` crate parses; `$INSUNITS` scaling (no Y flip: DXF is Y-up), layer names and filter, naming. |
| `dxf/entities.rs` | Each entity as a bulge polyline; INSERT expansion with its transform, depth limit and skip rules; SPLINE and ELLIPSE flattening. |
| `dxf/spline.rs` | Rational B-spline evaluation (de Boor) and sampling to the tolerance. |
| `dxf/stitch.rs` | Joining open segments whose ends meet, closing chains, dropping duplicates. |
| `lib.rs` | `ImportError`, `ImportWarning`. |

### `dc-cam` (`crates/dc-cam/src/`)

| Module | Contents |
| --- | --- |
| `profile.rs` | `generate_profile`; the direction rule `traversal_is_ccw`. |
| `passes.rs` | `pass_z_mm`: depth pass levels; refuses cuts below the stock bottom plus allowance. |
| `stock.rs` | `swept_bounds`, `stock_overhang`: does the cutter stay on the stock (#62). |
| `tabs.rs` | Tab placement along the tool-centre path, rect and smooth laps. |
| `toolpath.rs` | `Toolpath`, `Move`, `MoveKind`: project-space output. |
| `error.rs`, `warning.rs` | `CamError`; `Warning` with a stable `WarningCode`. |
| `test_support.rs` | `assert_safe`, the shared emitted-motion safety assertions (`test-support` feature). |

### `dc-gcode` (`crates/dc-gcode/src/`)

| Module | Contents |
| --- | --- |
| `post.rs` | Tests of every bundled definition version against the controller facts. |
| `emit.rs` | `emit`: toolpaths to `Program` (text, extension, pauses). |
| `transform.rs` | `MachineTransform`: project space to machine numbers; origin, axis map, units. |
| `format.rs` | Number formatting, the one place numbers become text. |
| `parse.rs` | Reads a program back as a controller would. |
| `safety.rs` | Checks S1–S7 on the parsed program and measures its extents. Every violation is an error. |
| `error.rs` | `ExportError`. |

### `dc-library` (`crates/dc-library/src/`)

| Module | Contents |
| --- | --- |
| `library.rs` | `Library`, `LibraryTool`, `LibraryToolId` (`lt<n>`), `LIBRARY_VERSION`; `load` (strict, validated), `save`, `add` (`Added` or `AlreadyPresent`), `replace`, `find` (by id, else unique name), `remove`. Validation rules ported from PureCutCNC `src/toolLibrary.ts`. |
| `fusion.rs` | Fusion 360 library import: `import(&[u8])` reads a `.tools` zip (the `zip` crate, deflate only) or `.json` into `FusionImport { tools: Vec<ImportedTool>, skipped: Vec<Skipped> }`, one tool per preset, with `ImportNote`s for unset values and `SkipReason`s; `is_fusion_library` tells a Fusion file from a DreamCarve one. |
| `error.rs` | `LibraryError`. |

### `dreamcarve` (`crates/dreamcarve/src/`)

| Module | Contents |
| --- | --- |
| `cli.rs` | Arguments (clap), exit codes, staged file writes, the date. |
| `init.rs` | `init`: import and the default project. |
| `export.rs` | `export`, and `build`, the whole pipeline without writing, which `check` shares. |
| `check.rs` | `check`. |
| `config.rs` | The user config directory and `config.toml`; `config path`. |
| `tools.rs` | `tools list`, `add`, `import`, `export`, `remove`: reads and writes `tools.json` and, for `add`, the project. |

Behaviour tests for the binary are in `crates/dreamcarve/tests/`.
`quick_start.rs` runs the commands of the
[quick start](../quick-start/first-cut.md) chapter, read from the chapter
itself.

## Outside the workspace crates

- `tools/check-headers`: the license header checker CI runs.
- `tools/doc-figures`: draws the user guide's geometry figures from real
  engine output (`cargo run -p doc-figures`); a test fails when a committed
  figure is stale.
- `tools/third-party-licenses`: gathers the license texts shipped with
  release binaries.
- `tools/parity/`: a Node script that drives a local PureCutCNC checkout to
  capture golden outputs. Not needed to build or test DreamCarve. See
  [Golden corpus](golden-corpus.md).
- `tests/golden/`: the captured PureCutCNC outputs that `cargo test`
  compares against.
- `fixtures/`: input files shared by tests and docs, such as `coaster.svg`.
- `docs/book/`: this site.

The design spec of each phase is the authority on where new code goes.
Rules for what may live in each crate are in `docs/rust-guidelines.md`,
"Crate boundaries".
