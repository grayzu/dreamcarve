# Crate map

DreamCarve is a Cargo workspace. Library crates carry a `dc-` prefix; the
command line binary is `dreamcarve`. Library crates never panic, print, or
do I/O: they take values and return `Result`s and warnings. The binary does
all file, terminal, clock and exit-code handling.

## How the pieces fit

```text
plaque.dxf ──dc-import──▶ definitions + instances ─┐
                                                   ├─▶ Project (dc-project) ──save/load──▶ plaque.dcv
         init defaults (dreamcarve) ───────────────┘
tools.json ──dc-library──▶ Tool ──tools add (dreamcarve)──▶ a copy in the project

plaque.dcv ──load + validate (dc-project)──▶ Project
           ──generate_profile / _pocket / _drill / _engrave (dc-cam)──▶ cut blocks
           ──sequence (dc-cam)──▶ Toolpath per operation (project space, mm, Y-up)
           ──order::check (dc-cam)──▶ operation-order warnings
           ──emit_programs (dc-gcode): arc fitting, canned cycles, one program per tool run or M6
           ──parse + safety pass (dc-gcode)──▶ plaque-1-T1.nc, plaque-2-T2.nc
```

Dependencies point one way, down this table. `dc-gcode` depends on
`dc-cam` only for the toolpath types it consumes; it trusts nothing a
generator says and works out each operation's material from the project
itself.

| Crate | Owns | Depends on |
| --- | --- | --- |
| `dreamcarve` | The command line. All file, terminal, clock and exit-code handling, the user config directory and the tool library file. | every `dc-` crate |
| `dc-library` | The tool library: its file format, load and save, adding, finding and removing tools, and Fusion 360 import. Bytes in, values out; the binary owns `tools.json`. | `dc-project` |
| `dc-gcode` | G-code emission, arc fitting, canned cycles, and the safety pass that parses every emitted program. | `dc-cam`, `dc-project`, `dc-geometry` |
| `dc-cam` | Toolpath generators, sequencing and the operation-order warnings. Pure functions from project and operation to toolpath plus warnings. | `dc-project`, `dc-geometry` |
| `dc-import` | Importers: SVG (flipped from Y-down into Y-up) and DXF (already Y-up, not flipped). | `dc-project`, `dc-geometry` |
| `dc-project` | The project data model, the bundled post-processor definitions, `.dcv` load and save, migrations, validation, undo history. | `dc-geometry` |
| `dc-geometry` | Points, polylines with bulge arcs, transforms, offsets, booleans, circle tests, flattening, nesting, distances. Knows nothing about tools or stock. | nothing internal |

The `test-support` features of `dc-project` and `dc-cam` carry fixture
builders and `assert_safe` for other crates' tests; they are
dev-dependencies only.

## Modules

### `dc-geometry` (`crates/dc-geometry/src/`)

| Module | Contents |
| --- | --- |
| `point.rs` | `Point`, `Point3`, `Vec2`, `Rect`; `EPS_MM` and `approx_eq`, used instead of `==` on floats; `MAX_COORDINATE_MM`, the 6 m bound the importers and validation share. |
| `polyline.rs` | `Polyline`, `Vertex`, `Segment`, `Arc`: lines and DXF-style bulge arcs; length, signed area, bounds, point at distance. |
| `contour.rs` | `Contour`: a closed polyline with known orientation (outer CCW, hole CW). |
| `transform.rs` | `Transform`: 2D affine, serialized as `[a, b, c, d, e, f]`. |
| `flatten.rs` | Bézier paths to polylines at a tolerance (kurbo); `FLATTEN_TOLERANCE_DEFAULT_MM`. |
| `offset.rs` | Parallel offset with round joins (cavalier_contours), arcs kept. |
| `region.rs` | `Region` and `nest`: loose contours to parts with holes. See [Shapes, holes, and islands](../user-guide/shapes-and-holes.md). |
| `boolean.rs` | Union, difference and intersection of regions (i_overlay), for pockets, rest machining and coverage. Arcs are flattened first; results are canonical (outers CCW, holes CW, a fixed start vertex and order). |
| `circle.rs` | `as_circle`, whether a closed outline is a drillable hole within a tolerance; `fit_circle_kasa`, the least-squares fit arc fitting starts from. |
| `distance.rs` | Shortest distances between points, segments and polylines, following arcs exactly; `nearest_point`, the point of a polyline nearest a point and how far along it that is. |

### `dc-project` (`crates/dc-project/src/`)

| Module | Contents |
| --- | --- |
| `model.rs` | `Project` and everything in it: `Stock`, `MachineOrigin`, `FeatureDefinition`, `FeatureInstance`, `Tool` (with its `number`), `Operation` and `ProfileOp`, `PocketOp`, `DrillOp`, `EngraveOp`; `PlacedTab` (the project's `tabs`), `TabSpec` (a Profile's tab request), `Entry`, `Lead`, `CornerRelief`, `MachiningOrder`. |
| `post_processor.rs` | `PostProcessor`, the controller dialect; `PostProcessorRef`, what a project stores (a bundled definition's id and version, and the user's settings such as `tool_change_mode` and `use_canned_cycles`); and the bundled definitions, by id and version. |
| `definitions/` | `<id>/v<n>.json` for grblHAL, GRBL, LinuxCNC, Mach3, UCCNC and generic, one file per definition version, compiled in and never edited once released, and a README with their verification against controller sources. |
| `ids.rs` | `DefinitionId`, `InstanceId`, `ToolId`, `OperationId`, `TabId` newtypes. |
| `serialize.rs` | `save`, `load` (strict: unknown and missing fields fail), `Loaded`, `LoadError`. |
| `layout.rs` | The saved file's layout: serde_json's pretty printing with each vertex on one line (#76). |
| `locate.rs` | `SourceMap`: the line and column of every value in a `.dcv`'s text, so messages read `plaque.dcv:10:21:` (#76). |
| `migrate.rs` | `CURRENT_VERSION` (11) and the raw-JSON migration steps from older versions. |
| `validate.rs` | `validate`, `Problem`, `Severity`: every problem at once, errors and warnings. |
| `history.rs` | `History`: snapshot undo and redo. Nothing drives it until the GUI. |
| `test_support.rs` | Fixture builders such as `sample_project()` (`test-support` feature). |

### `dc-import` (`crates/dc-import/src/`)

| Module | Contents |
| --- | --- |
| `lib.rs` | `ImportError`, `ImportWarning`. |
| `svg.rs` | `import_svg`: usvg parses; each path becomes one definition and one instance, flipped to Y-up; circular arcs become bulges, other curves are flattened. |
| `svg/arc.rs` | Recognising usvg's Bézier pieces of circular arcs (circles, equal-radius ellipses and `A` commands, rounded-rect corners) exactly, and joining them back into arcs (#70). |
| `dxf.rs` | `import_dxf`, `DxfReadError`: `$INSUNITS` scaling (no Y flip: DXF is Y-up), layer names and filter, naming. |
| `dxf/groups.rs` | The group-code reader: ASCII and binary DXF to typed (code, value) pairs; UTF-8, code page and `\U+XXXX` text. |
| `dxf/code_page.rs` | The single-byte Windows code pages (874, 1250–1258) that text before R2007 is read in, by `$DWGCODEPAGE` (#209). |
| `dxf/document.rs` | The header units, version and code page, blocks and entities from the pairs; VERTEX rows attached to their POLYLINE. |
| `dxf/entities.rs` | Each entity as a bulge polyline through its OCS (extrusion direction); INSERT and MINSERT expansion with the transform, depth limit and skip rules; SPLINE and ELLIPSE flattening. |
| `dxf/spline.rs` | Rational B-spline evaluation (de Boor) and sampling to the tolerance. |
| `dxf/stitch.rs` | Joining open segments whose ends meet, closing chains, dropping duplicates. |

### `dc-cam` (`crates/dc-cam/src/`)

| Module | Contents |
| --- | --- |
| `lib.rs` | The crate's public surface: the four `generate_*` functions, `sequence`, `footprint`, the toolpath and warning types. |
| `block.rs` | `CutBlock`: one contour, open path, drill point or relief notch at one pass depth, what a generator produces before ordering (D10). |
| `sequence.rs` | `sequence`: orders blocks by machining order, containment precedence and nearest neighbour, writes the links (stay down or retract), and produces the `Toolpath`. |
| `order.rs` | `order::check`: the `cut_inside_freed_part`, `cut_inside_tab_held_part` and `tool_change_avoidable` warnings between operations. Never reorders. |
| `profile.rs` | `generate_profile` on blocks; the direction rule `traversal_is_ccw`. |
| `profile/entries.rs` | Each Profile pass's entry, placed in the waste domain, with and without the tab keep-outs. |
| `profile/waste.rs` | The Profile waste domain: where an entry or lead may go because the material there is cut away anyway. |
| `pocket.rs` | `generate_pocket`: regions and islands, allowance, wall pass, direction (`ring_is_ccw`), entries and relief hooks. |
| `pocket/rings.rs` | `inset_region` and `RingTree`: the offset ring tree and its inner-first walk. |
| `pocket/raster.rs` | `raster_lines` and `order_nearest_first`: the raster strategy. |
| `pocket/rest.rs` | Rest machining: the leftover of an earlier Pocket, from its tool and allowance only (S14). |
| `drill.rs` | `generate_drill`: circle targets, simple, peck and chip-break cycles, dwell, retract height, `DrillHole` records for canned cycles (S15). |
| `engrave.rs` | `generate_engrave`: every path traced with the tool centre on the line; `alternate_direction`. |
| `entry.rs` | Helix and zigzag ramp placement and descent in a pass's clearance regions, the helix → ramp → plunge fallback, and the plunge-rate feed scale (S10); `helix_is_ccw`. |
| `entry/wall.rs` | Entry on a Profile's wall: the ramp anchored on the entry point, and the On line ramp along the path. |
| `lead.rs` | Lead-in and lead-out arcs: start-point ranking, shrinking, dropping. |
| `relief.rs` | Dogbone and T-bone corner relief on Pocket and Profile. |
| `footprint.rs` | `footprint`: the area a toolpath's cutting moves sweep, for S9 and the coverage tests. |
| `passes.rs` | `pass_z_mm`: depth pass levels; refuses cuts below the stock bottom plus allowance. |
| `overlap.rs` | The check that two shapes' tool paths in one Profile do not cut into each other (D6). |
| `stock.rs` | `swept_bounds`, `stock_overhang`: does the cutter stay on the stock (#62). |
| `tabs.rs` | Tab spans along the tool-centre path, each tab with its own width, height and shape; merging; rect and smooth laps. |
| `placed_tabs.rs` | Placed tabs (#105): `place` (a tab request's layout as placed tabs), `place_on_outlines` (`tabs place --at`), `place_requests`, `crossed_by`, and `check`, the `tab_not_crossed` warning. |
| `toolpath.rs` | `Toolpath`, `Move`, `MoveKind` (including `Dwell`): project-space output. |
| `error.rs`, `warning.rs` | `CamError`; `Warning` with a stable `WarningCode`. |
| `test_support.rs` | `assert_safe`, the shared emitted-motion safety assertions (`test-support` feature). |

The test modules `entry/profile_tests.rs` and `lead/tests.rs` sit beside
the code they test.

### `dc-gcode` (`crates/dc-gcode/src/`)

| Module | Contents |
| --- | --- |
| `emit.rs` | `emit_programs` and `emit`: toolpaths to `Program`s (text, extension, pauses), one per tool run or one with `M6`; canned cycles where the controller's motion for them is the toolpath. |
| `arcfit.rs` | Arc fitting: runs of straight moves on one circle written as `G2`/`G3` within `arc_fit_tolerance_mm`, rejected when rounding would fail the controller's check (S12). |
| `canned.rs` | What a controller does for a `G81`, `G82`, `G83` or `G73` line, used by the parser and the emitter (S11). |
| `transform.rs` | `MachineTransform`: project space to machine numbers; origin, axis map, units. |
| `format.rs` | Number formatting, the one place numbers become text. |
| `parse.rs` | Reads a program back as a controller would, canned cycles expanded into their moves. |
| `safety.rs` | Checks S1–S7 and S9–S12, S14 and S15 on the parsed program and measures its extents. Every violation is an error. (S8 is validation's warnings; S13, the order invariants, is tested in `dc-cam`.) |
| `material.rs` | S9: every operation's cuts stay in its own material, worked out from the project. |
| `post.rs` | Tests of every bundled definition version against the controller facts. |
| `error.rs` | `ExportError`. |

### `dc-library` (`crates/dc-library/src/`)

| Module | Contents |
| --- | --- |
| `lib.rs` | The crate's surface: `load`, `save`, `add`, `find`, `remove`, and `fusion`. |
| `library.rs` | `Library`, `LibraryTool`, `LibraryToolId` (`lt<n>`), `LIBRARY_VERSION`; `load` (strict, validated), `save`, `add` (`Added` or `AlreadyPresent`), `replace`, `find` (by id, else unique name), `remove`. Validation rules ported from PureCutCNC `src/toolLibrary.ts`. |
| `fusion.rs` | Fusion 360 library import: `import(&[u8])` reads a `.tools` zip (the `zip` crate, deflate only) or `.json` into `FusionImport { tools: Vec<ImportedTool>, skipped: Vec<Skipped> }`, one tool per preset, with `ImportNote`s for unset values and `SkipReason`s; `is_fusion_library` tells a Fusion file from a DreamCarve one. |
| `error.rs` | `LibraryError`. |

### `dreamcarve` (`crates/dreamcarve/src/`)

| Module | Contents |
| --- | --- |
| `main.rs` | Entry point: runs the command and maps its result to an exit code. |
| `cli.rs` | Arguments (clap), exit codes, staged file writes, the date. |
| `output.rs` | `outln!`, `out!`, `errln!`: all printing; a closed stdout pipe stops output and keeps the exit code (#216). |
| `init.rs` | `init`: SVG or DXF import and the default project; the DXF-only `--units`, `--join-tolerance` and `--layer`. |
| `export.rs` | `export`, and `build`, the whole pipeline without writing, which `check` shares; `--post`, `--one-program`, `--programs-per-tool`. |
| `check.rs` | `check`. |
| `config.rs` | The user config directory (`DREAMCARVE_CONFIG_DIR` or the platform's), `config.toml`, and `config path`. |
| `tools.rs` | `tools list`, `add`, `import`, `export`, `remove`: reads and writes `tools.json` and, for `add`, the project. |
| `tabs.rs` | `tabs place`, `list`, `clear`: the project's placed tabs. |
| `clock.rs` | UTC dates from the system clock, with no date crate; `build.rs` includes it for `--version`. |

Behaviour tests for the binary are in `crates/dreamcarve/tests/`: `cli.rs`
for every command, with committed programs in `tests/expected/`, and
`quick_start.rs`, which runs the commands of the quick start chapters,
read from the chapters themselves.

## Outside the workspace crates

- `tools/check-headers`: the license header checker CI runs.
- `tools/doc-figures`: draws the user guide's geometry figures from real
  engine output (`cargo run -p doc-figures`); a test fails when a committed
  figure is stale.
- `tools/third-party-licenses`: gathers the license texts shipped with
  release binaries.
- `tools/parity/`: a Node script, not a Cargo crate, that drives a local
  PureCutCNC checkout to capture golden outputs. Not needed to build or
  test DreamCarve. See [Golden corpus](golden-corpus.md).
- `tests/golden/`: the captured PureCutCNC outputs, with each case's
  DreamCarve project, that `cargo test` compares against.
- `fixtures/`: input files the user docs link to, such as `coaster.svg`.
- `tests/fixtures/`: input files for tests: the plaque in SVG and DXF,
  DXF entity cases, and hand-written Fusion libraries.
- `docs/book/`: this site.

The design spec of each phase is the authority on where new code goes.
Rules for what may live in each crate are in `docs/rust-guidelines.md`,
"Crate boundaries".
