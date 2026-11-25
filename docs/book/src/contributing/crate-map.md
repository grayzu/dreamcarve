# Crate map

DreamCarve is a Cargo workspace. Library crates carry a `dc-` prefix; the
command line binary is `dreamcarve`, and the app (phase 4, in progress)
is `dreamcarve-app`. Library crates never panic, print, or do I/O: they take
values and return `Result`s and warnings. The binaries do the terminal,
clock and exit-code handling; the file I/O they share (staged writes, the
config directory, `config.toml`, `gui.toml`, `tools.json`) is
`dreamcarve-shell`'s, a
library that may do I/O and is therefore not a `dc-` crate.

## How the pieces fit

```text
plaque.dxf ──dc-import──▶ definitions + instances ─┐
photo.png ──dc-import::bitmap──▶ Bitmap ───────────┤
                                                   ├─▶ Project (dc-project) ──save/load──▶ plaque.dcv
   project_from::drawing / ::picture (dc-job) ─────┘
logo.svg ──dc-import::prep (image-prep)──▶ logo-inlay.svg, a cleaner drawing for init
tools.json ──dc-library──▶ Tool ──project_copy (tools add, the app's Add to project)──▶ a copy in the project

plaque.dcv ──load (dc-project)──▶ Project ──dc-job::build (both shells call this)──▶ programs + findings
   build = validate (dc-project)
           ──generate_profile / _pocket / _drill / _bore / _engrave / _vcarve / _inlay_board / _photo (dc-cam)──▶ cut blocks
           ──sequence (dc-cam)──▶ ToolRuns per operation, a Toolpath each (project space, mm, Y-up)
           ──order::check + placed_tabs::check (dc-cam)──▶ operation-order and tab warnings
           ──emit_programs (dc-gcode): arc fitting, canned cycles, one program per tool run or M6
           ──parse + safety pass, S1–S19 (dc-gcode)──▶ plaque-1-T2.nc, plaque-2-T1.nc
   dc-job also names the programs, writes the summary lines and draws the preview SVG
```

Dependencies point one way, down this table. `dc-gcode` depends on
`dc-cam` only for the toolpath types it consumes; it trusts nothing a
generator says and works out each operation's material from the project
itself. The GUI crates (`dc-app`, `dreamcarve-app`, phase 4) depend on
`dc-cam` and `dc-gcode` only through `dc-job`, so the program the app
writes is the CLI's (phase 4 functional spec, S20).

| Crate | Owns | Depends on |
| --- | --- | --- |
| `dreamcarve` | The command line: argument parsing, printing, the clock, exit codes, and the `tabs`, `image` and `image-prep` commands' flags. Thin: the pipeline is `dc-job`'s and the shared file I/O `dreamcarve-shell`'s. | every `dc-` crate but `dc-app`, `dreamcarve-shell` |
| `dreamcarve-app` | The app (phase 4): the egui window on wgpu, its header, rail and status bar, the five steps' panels, the Inspector and the sheets, the Viewport renderer, the widgets and theme, the file dialogs, the keyboard shortcut table, and the worker thread that runs generation. Draws what `dc-app` computes and turns input into its commands. | `dc-app`, `dc-job`, `dc-import`, `dc-library`, `dc-project`, `dc-geometry`, `dreamcarve-shell` |
| `dreamcarve-shell` | The file I/O both binaries share: the staged write that puts every file of an export in place or none, the user config directory, `config.toml`, `gui.toml` and the tool library file `tools.json`. Reads and writes files; prints nothing. | `dc-job`, `dc-library`, `dc-project` |
| `dc-app` | The app's core (phase 4): one `History` of `Project` snapshots with a label per step, the `Command` enum whose `apply` makes the next snapshot, the selection, the view settings, the generation bookkeeping (what is pending, stale, finished), the string table and its generated `StrKey`, units formatting and parsing, the preferences record, the project tools' editing rules, the Tool Library window's model and the Import drawing sheet's, the tool model of phase 5 (`tool`: the tool in hand, the events the window hands it, the tool bar it shows and the draft it wants drawn; snapping; the fonts on hand), and the headless test harness. No egui, no I/O. | `dc-job`, `dc-import`, `dc-library`, `dc-project`, `dc-geometry` |
| `dc-job` | The job pipeline: `build` (validate, generate, check, emit) and its two halves `generate_operation` and `finish`; `Findings` with tiers; program names; the export summary's lines and the setup sheet; the preview SVG and its marks; a project from a drawing or a picture; a drawing or a picture added to a project; the artwork checks of a drawing and of a project's shapes; the `image-prep` report's lines; placed tabs. Re-exports the `dc-cam` and `dc-gcode` types a shell needs. | `dc-cam`, `dc-gcode`, `dc-import`, `dc-project`, `dc-geometry` |
| `dc-library` | The tool library: its file format, load and save, adding, finding and removing tools, Fusion 360 import, importing a file with the per-tool report, and the copy a project embeds. Bytes in, values out; the shells own `tools.json`, through `dreamcarve-shell`. | `dc-project` |
| `dc-gcode` | G-code emission, arc fitting, canned cycles, and the safety pass that parses every emitted program and re-derives each operation's material from the project, the carves' cones (S17), countersinks (S18) and photo grooves (S19) included. | `dc-cam`, `dc-project`, `dc-geometry` |
| `dc-cam` | Toolpath generators (Profile, Pocket, Drill and countersink, Bore, Engrave, V-Carve and its clearing passes, Inlay, Photo V-Carve), tool runs, sequencing and the operation-order warnings. Pure functions from project and operation to toolpaths plus warnings. | `dc-project`, `dc-geometry` |
| `dc-sim` | Material-removal simulation (phase 6): the stock as a grid of heights, the shapes cutters have, and the cut of one move into the grid, exact for a move that falls as it goes. A reader: it knows nothing above `dc-project`, and no generator or post-processor reads it (S22). `dc-cam`'s tests use its kernel. | `dc-project`, `dc-geometry` |
| `dc-text` | Text (phase 5): a font read from its bytes, the fonts that travel with the app (compiled in, each with its licence), the single-line font *Simple Stroke*, and the outlines of laid-out text, counters as holes. Reads no file and lists no system font. | `dc-geometry` |
| `dc-import` | Importers: SVG (flipped from Y-down into Y-up), DXF (already Y-up, not flipped) and pictures (PNG and JPEG to the 8-bit grayscale a bitmap stores, top row at the highest Y); image prep and the import checks. | `dc-project`, `dc-geometry` |
| `dc-project` | The project data model, the bundled post-processor definitions, `.dcv` load and save, migrations, validation, undo history, and the rules validation, generators and the safety pass must share: a tool's derived profile, the countersink's depth, the cleared start plane, the male Inlay's band, a stored bitmap's checked decoding. | `dc-geometry` |
| `dc-geometry` | Points, polylines with bulge arcs, transforms, offsets, booleans, circle tests, flattening, nesting, distances; the medial axis, the carve's region constructions, fill resolution, widths, simplification, and a carving bit's profile with its exact least room. Knows nothing about projects, tools or stock: a bit is only its profile. | nothing internal |

The `test-support` features of `dc-project` and `dc-cam` carry fixture
builders and `assert_safe` for other crates' tests, and `dc-app`'s carries
the `Harness` that drives the app headless; they are dev-dependencies
only.

## Modules

The tables list each module's code. A small module keeps its unit tests
in a `#[cfg(test)] mod tests` at its foot. A module whose tests outgrew
it (most of `dc-cam`, and the large modules of `dc-project`, `dc-gcode`
and `dc-geometry`) declares `#[cfg(test)] mod tests;` and keeps them in
`<module>/tests.rs`: `profile.rs` has `profile/tests.rs`, and `inlay.rs`
the files under `inlay/tests/`. A few test modules have a name of their
own (`entry/profile_tests.rs`, `profile/defect_tests.rs`).

### `dc-geometry` (`crates/dc-geometry/src/`)

| Module | Contents |
| --- | --- |
| `point.rs` | `Point`, `Point3`, `Vec2`, `Rect`; `EPS_MM` and `approx_eq`, used instead of `==` on floats; `MAX_COORDINATE_MM`, the 6 m bound the importers and validation share. |
| `polyline.rs` | `Polyline`, `Vertex`, `Segment`, `Arc`: lines and DXF-style bulge arcs; length, signed area, bounds, point at distance. |
| `contour.rs` | `Contour`: a closed polyline with known orientation (outer CCW, hole CW). |
| `transform.rs` | `Transform`: 2D affine, serialized as `[a, b, c, d, e, f]`; `mirror_x_about`, the male Inlay's mirror. |
| `flatten.rs` | Bézier paths to polylines at a tolerance (kurbo); `FLATTEN_TOLERANCE_DEFAULT_MM`. |
| `offset.rs` | Parallel offset with round joins (cavalier_contours), arcs kept. |
| `region.rs` | `Region` and `nest`: loose contours to parts with holes. See [Shapes, holes, and islands](../user-guide/shapes-and-holes.md). |
| `boolean.rs` | Union, difference and intersection of regions (i_overlay), for pockets, rest machining and coverage. Arcs are flattened first; results are canonical (outers CCW, holes CW, a fixed start vertex and order). |
| `circle.rs` | `as_circle`, whether a closed outline is a drillable hole within a tolerance; `fit_circle_kasa`, the least-squares fit arc fitting starts from. |
| `distance.rs` | Shortest distances between points, segments and polylines, following arcs exactly (an arc past a half turn is split before it is crossed, #686); `nearest_point`, the point of a polyline nearest a point and how far along it that is. |
| `self_band.rs` | `SelfBand`: how close two stretches of one line come where they lie far apart along it (#221). |
| `medial.rs`, `medial/` | The medial axis of a region, ported from PureCutCNC's `vcarveMedial`: boundary sampling at a resolution, `delaunator` triangulation, each node's exact clearance (`grid.rs`), the axis round a line's end or a wedge (`feature.rs`), pruning and corner reach. |
| `carve.rs` | The region constructions a carve and the safety pass share (security review M3): `floor_regions`, `surround`, `rounded_band`, `closed_band`, `drawn_band`, `band_clearance`, `band_on_board`, `wall_tolerance_mm`. |
| `merge.rs` | `merge_near_parts`: parts of a design that nearly touch merged into one shape, so a female and its male agree (#479). |
| `fill.rs` | Image prep's fill resolution: `resolve_fill` under an SVG fill rule, `merge_fills`, `ink_outline`, `spaces_between` (regions mode). |
| `width.rs` | `is_narrower_than`, `hole_is_narrower_than` (one erosion), `local_widths` and `narrowest_gap` for the image-prep report. |
| `simplify.rs` | Douglas-Peucker simplification of closed contours, for image prep and pocket rings (#474). |
| `profile.rs`, `profile/` | `BitProfile`, a carving bit's cross-section (a V-bit, or a tapered ball nose's ball, cone and cylinder) and its inverse, and `least_room`, the exact least room of a straight cut against the walls that S17 and the generators share; `roots.rs` isolates a polynomial's roots, `stretch.rs` one stretch against one wall piece. |

### `dc-project` (`crates/dc-project/src/`)

| Module | Contents |
| --- | --- |
| `model.rs` | `Project` and everything in it: `Stock`, `MachineOrigin`, `FeatureDefinition`, `FeatureInstance`, `Bitmap`, `Tool` (with its `number`) and `ToolKind` (a V-bit, a tapered ball nose and the rest), `Operation` and `ProfileOp`, `PocketOp`, `DrillOp` (with `DrillCycle::Countersink`), `BoreOp`, `EngraveOp`, `VCarveOp`, `InlayOp` (with `MaleBoard` and `InlayCutOut`), `PhotoVCarveOp`; `ClearingPass`, `Board`; `PlacedTab` (the project's `tabs`), `TabSpec` (a Profile's tab request), `Entry`, `Lead`, `CornerRelief`, `MachiningOrder`. |
| `post_processor.rs` | `PostProcessor`, the controller dialect; `PostProcessorRef`, what a project stores (a bundled definition's id and version, and the user's settings such as `tool_change_mode` and `use_canned_cycles`); and the bundled definitions, by id and version. |
| `definitions/` | `<id>/v<n>.json` for grblHAL, GRBL, LinuxCNC, Mach3, UCCNC and generic, one file per definition version, compiled in and never edited once released, and a README with their verification against controller sources. |
| `ids.rs` | `DefinitionId`, `InstanceId`, `ToolId`, `OperationId`, `TabId`, `BitmapId` newtypes. |
| `tool.rs` | What a `Tool` derives from its values: `carve_profile` (the `dc_geometry::BitProfile` a carve and S17 read), the tip and cut radii, the cutting height and `cutting_limit` (flutes or cap), and `unset_values`, what a tool still needs. |
| `units_input.rs` | Lengths and feeds written with a unit (`"1/4 in"`, `"60 in/min"`), read into millimetres; `LENGTH_DECIMALS`, the rounding importers share. |
| `bitmap.rs` | Reading a bitmap's stored PNG (format 27): the header alone for loading and validation, the pixels under checked limits (`MAX_BITMAP_SIDE_PX`; security review M4). |
| `countersink.rs` | `countersink_depth_mm`, the depth a countersink's mouth and V-bit derive, shared by validation, the generator and S18. |
| `project.rs` | `cleared_start_plane`: which earlier Pocket or V-Carve floor holds a carve's start plane, shared by validation and `dc-cam`'s start depth checks. |
| `inlay.rs` | The male Inlay's band, shared by the generator, validation and the safety pass: `male_band`, `edge_pass`, `band_edge`, the drawn boundary's regions and problems; `Project::on_male_board`. |
| `serialize.rs` | `save`, `load` (strict: unknown and missing fields fail), `Loaded`, `LoadError`. |
| `layout.rs` | The saved file's layout: serde_json's pretty printing with each vertex on one line (#76). |
| `locate.rs` | `SourceMap`: the line and column of every value in a `.dcv`'s text, so messages read `plaque.dcv:10:21:` (#76). |
| `migrate.rs` | `CURRENT_VERSION`, the format this build writes ([Versions](../user-guide/project-file.md#versions)), and the raw-JSON migration steps from older versions, with the notes an upgrade prints (`migrate_with_notes`). |
| `validate.rs` | `validate`, `Problem`, `Severity`: every problem at once, errors and warnings. |
| `history.rs` | `History`: snapshot undo and redo. Nothing drives it until the GUI. |
| `test_support.rs` | Fixture builders such as `sample_project()` (`test-support` feature). |

### `dc-import` (`crates/dc-import/src/`)

| Module | Contents |
| --- | --- |
| `lib.rs` | `ImportError`, `ImportWarning`, each warning's stable `code()`. |
| `round.rs` | Rounding of imported coordinates to `LENGTH_DECIMALS` places, bulges kept (#389). |
| `svg.rs` | `import_svg`: usvg parses; each path becomes one definition and one instance, flipped to Y-up; circular arcs become bulges, other curves are flattened. |
| `svg/arc.rs` | Recognising usvg's Bézier pieces of circular arcs (circles, equal-radius ellipses and `A` commands, rounded-rect corners) exactly, and joining them back into arcs (#70). |
| `dxf.rs` | `import_dxf`, `DxfReadError`, `DxfLayer`: `$INSUNITS` scaling (no Y flip: DXF is Y-up), layer names and filter, naming, and the file's layer list with each layer's closed, open and unread counts. |
| `dxf/groups.rs` | The group-code reader: ASCII and binary DXF to typed (code, value) pairs; UTF-8, code page and `\U+XXXX` text. |
| `dxf/code_page.rs` | The single-byte Windows code pages (874, 1250–1258) that text before R2007 is read in, by `$DWGCODEPAGE` (#209). |
| `dxf/document.rs` | The header units, version and code page, blocks and entities from the pairs; VERTEX rows attached to their POLYLINE. |
| `dxf/entities.rs` | Each entity as a bulge polyline through its OCS (extrusion direction); INSERT and MINSERT expansion with the transform, depth limit and skip rules; SPLINE and ELLIPSE flattening. |
| `dxf/spline.rs` | Rational B-spline evaluation (de Boor) and sampling to the tolerance. |
| `dxf/stitch.rs` | Joining open segments whose ends meet, closing chains, dropping duplicates. |
| `dxf/hatch.rs` | HATCH boundary paths as shapes (#668): polyline and edge paths in the hatch's coordinate system, bounded before they are built. |
| `bitmap.rs`, `bitmap/` | Pictures (format 27): PNG and JPEG decoded under size limits checked before any pixel is (`png.rs`, `jpeg.rs`), converted to 8-bit grayscale by luminance with transparency on white (`gray.rs`), and encoded as the PNG a bitmap stores. |
| `prep.rs`, `prep/` | Image prep (sub-phase 3.4): `walk.rs`, which paths an SVG draws (PureCutCNC's paint-intent rule); `findings.rs`, the import checks `init` and `check` print; `stroke.rs`, strokes to areas with a vertex budget; `run.rs`, the `prepare` pipeline; `dxf.rs`, a DXF's closed shapes as its input (#669); `write.rs`, the SVG it writes, read back before it is returned; `report.rs`, the report's data; `defaults.rs`, the calibrated defaults. |

### `dc-cam` (`crates/dc-cam/src/`)

| Module | Contents |
| --- | --- |
| `lib.rs` | The crate's public surface: the `generate_*` functions, `sequence`, `footprint`, `ToolRun`, the toolpath and warning types. |
| `run.rs` | `ToolRun` and `RunLabel`: an operation's toolpaths, one per tool (a carve's clearing passes, an Inlay's cut-out) and per board (an Inlay's female and male). |
| `block.rs` | `CutBlock`: one contour, open path, drill point or relief notch at one pass depth, what a generator produces before ordering (D10). |
| `context.rs`, `targets.rs` | What the generators share before they cut: the checks of the tool, its rates, the safe Z, the step-down, the thickness and the depth, each generator calling them in its own order; and `target_profiles`, the walk over an operation's targets. |
| `sequence.rs` | `sequence`: orders blocks by machining order, containment precedence and nearest neighbour, writes the links (stay down or retract), and produces the `Toolpath`. |
| `order.rs` | `order::check`: the `cut_inside_freed_part`, `cut_inside_tab_held_part` and `tool_change_avoidable` warnings between operations. Never reorders. |
| `profile.rs` | `generate_profile` on blocks; the direction rule `traversal_is_ccw`. |
| `profile/entries.rs` | Each Profile pass's entry, placed in the waste domain, with and without the tab keep-outs. |
| `profile/waste.rs` | The Profile waste domain: where an entry or lead may go because the material there is cut away anyway. |
| `profile/last_pass.rs`, `profile/tab_descent.rs`, `profile/reached.rs`, `profile/start.rs` | A Profile's last pass stock to leave and its stock path (#284), the last pass's descent off a tall tab (#348), the tabs a lap's bit reaches on other outlines (#362), and the start point (#285). |
| `pocket.rs` | `generate_pocket`: regions and islands, stock to leave, wall pass, direction (`ring_is_ccw`), entries and relief hooks. |
| `pocket/rings.rs` | `inset_region` and `RingTree`: the offset ring tree and its inner-first walk. |
| `pocket/raster.rs` | `raster_lines` and `order_nearest_first`: the raster strategy. |
| `pocket/rest.rs` | Rest machining: the leftover of an earlier Pocket, from its tool and stock to leave only (S14). |
| `bore.rs` | `generate_bore` (format 18): circle targets, a helix of the hole's radius less the tool's at `pitch_mm` a turn, one circle at the depth, the holes it skips (too small, or leaving a core). |
| `drill.rs` | `generate_drill`: circle targets, simple, peck and chip-break cycles, dwell, retract height, `DrillHole` records for canned cycles (S15); the countersink (format 21), one plunge at each centre, always plain moves. |
| `vcarve.rs`, `vcarve/` | `generate_vcarve` (format 20): `medial.rs`, the medial strategy's chains, depths and links (ported from PureCutCNC, with the H2 link rule); `offset.rs`, the offset rings; `floor.rs`, the flat floor's trace and the V-bit floor; `clearing.rs`, the clearing passes, one `ToolRun` each; `cone.rs`, the walls and the exact cone test the carve runs on its own toolpath; `written.rs`, a corner's groove on the written grid (#499). On the bit's profile, so a tapered ball nose carves too. |
| `inlay.rs` | `generate_inlay` and `generate_inlay_board`: the female as a V-Carve with a flat depth, the male's mirrored band, its edge pass and its cut-out, each run tagged with its board. Corbin's Workshop's method; not a port. |
| `photo.rs` | `generate_photo` (format 27): a bitmap's grooves, their depths from the pixels, the hatch set and the links at the retract height. Designed from RasterCarve's and VCarve's documented behaviour; not a port. |
| `engrave.rs` | `generate_engrave`: every path traced with the tool centre on the line; `alternate_direction`. |
| `entry.rs` | Helix and zigzag ramp placement and descent in a pass's clearance regions, the helix → ramp → plunge fallback, and the plunge-rate feed scale (S10); `helix_is_ccw`. |
| `entry/wall.rs` | Entry on a Profile's wall: the ramp anchored on the entry point, and the On line ramp along the path. |
| `lead.rs` | Lead-in and lead-out arcs: start-point ranking, shrinking, dropping. |
| `relief.rs` | Dogbone and T-bone corner relief on Pocket and Profile. |
| `footprint.rs` | `footprint`: the area a toolpath's cutting moves sweep, for S9 and the coverage tests. |
| `passes.rs` | `pass_z_mm`: depth pass levels; refuses cuts below the stock bottom plus allowance. |
| `overlap.rs` | The check that two shapes' tool paths in one Profile do not cut into each other (D6). |
| `stock.rs` | `swept_bounds`, `stock_overhang`: does the cutter stay on the stock (#62). |
| `tabs.rs` | Tab spans along the tool-centre path, each tab with its own width, height and shape; merging; rect and smooth laps. `tabs/corners.rs`: tabs slid off corners (#285). |
| `placed_tabs.rs` | Placed tabs (#105): `place` (a tab request's layout as placed tabs), `place_on_outlines` (`tabs place --at`), `place_requests`, `crossed_by`, `on_holes`, `off_outline_mm`, and `check`, the `tab_not_crossed` and `tab_off_outline` (#300) warnings. |
| `toolpath.rs` | `Toolpath`, `Move`, `MoveKind` (including `Dwell`): project-space output. |
| `error.rs`, `warning.rs` | `CamError`; `Warning` with a stable `WarningCode`. |
| `test_support.rs` | `assert_safe`, the shared emitted-motion safety assertions, and `assert_run_safe` for a tool run (`test-support` feature). |
| `heightfield.rs` | A heightfield simulation of a carve's programs, exact for V-bit, tapered ball nose and end mill, for the tests that measure how a plug seats (`test-support` feature). Its closed forms are `dc-sim`'s since phase 6. |

### `dc-gcode` (`crates/dc-gcode/src/`)

| Module | Contents |
| --- | --- |
| `emit.rs` | `emit_programs` and `emit`: toolpaths to `Program`s (text, extension, pauses), one per tool run or one with `M6`; canned cycles where the controller's motion for them is the toolpath. |
| `arcfit.rs` | Arc fitting: runs of straight moves on one circle written as `G2`/`G3` within `arc_fit_tolerance_mm`, rejected when rounding would fail the controller's check (S12). |
| `canned.rs` | What a controller does for a `G81`, `G82`, `G83` or `G73` line, used by the parser and the emitter (S11). |
| `transform.rs` | `MachineTransform`: project space to machine numbers; origin, axis map, units. |
| `format.rs` | Number formatting, the one place numbers become text. |
| `parse.rs` | Reads a program back as a controller would, canned cycles expanded into their moves. |
| `safety.rs` | Checks S1–S7, S9–S12 and S14–S19 on the parsed program and measures its extents. Every violation is an error. (S8 is validation's warnings; S13, the order invariants, is tested in `dc-cam`.) |
| `material.rs`, `material/` | S9: every operation's cuts stay in its own material, worked out from the project, never from a generator. `cone.rs`, S17: a carve's bit, on its profile, cuts nothing below the final surface and keeps its shank out; `countersink.rs`, S18: each countersink plunge at its hole's centre to the derived depth; `photo.rs`, S19: a Photo V-Carve's grooves stay inside the picture and its depth range. |
| `ascii.rs` | The hand-written table that transliterates comment text to printable ASCII. |
| `estimate.rs` | `estimate`: a program's cycle-time estimate (feed time, dwells, rapid distance, stops), read from its text as `parse.rs` reads it. |
| `post.rs` | Tests of every bundled definition version against the controller facts. |
| `error.rs` | `ExportError`. |

### `dc-library` (`crates/dc-library/src/`)

| Module | Contents |
| --- | --- |
| `lib.rs` | The crate's surface: `load`, `save`, `add`, `find`, `remove`, `project_copy`, `fusion` and `import`. |
| `library.rs` | `Library`, `LibraryTool`, `LibraryToolId` (`lt<n>`), `LIBRARY_VERSION`; `load` (strict, validated), `save`, `add` (`Added` or `AlreadyPresent`), `replace`, `find` (by id, else unique name), `remove`; `project_copy` and `next_project_tool_id`, the copy `tools add` and the app put in a project under the next `t<n>`. Validation rules ported from PureCutCNC `src/toolLibrary.ts`. |
| `fusion.rs` | Fusion 360 library import: `import(&[u8])` reads a `.tools` zip (the `zip` crate, deflate only) or `.json` into `FusionImport { tools: Vec<ImportedTool>, skipped: Vec<Skipped> }`, one tool per preset, with `ImportNote`s for unset values and `SkipReason`s, a ball-tipped tapered mill as a tapered ball nose (#682); `is_fusion_library` tells a Fusion file from a DreamCarve one. |
| `import.rs` | `read` (a Fusion or DreamCarve file, by content) and `merge` (each tool added or replaced): the library after `tools import` and its `ImportReport`, whose `lines` and `summary` are what the CLI prints and the app's Tool Library shows. Moved from the CLI's `tools.rs`. |
| `error.rs` | `LibraryError`. |

### `dc-job` (`crates/dc-job/src/`)

| Module | Contents |
| --- | --- |
| `lib.rs` | The crate's surface, and the re-exports of the `dc-cam` and `dc-gcode` types a shell needs (`Toolpath`, `Move`, `Warning`, `EmitOp`, `Program`, `ProgramSplit`, `Estimate`, `Extents`, the error types). |
| `build.rs` | `build` and `build_board`, the whole pipeline without writing, which `export` and `check` share; its halves for the app, `validation`, `generate_operation` and `finish`, and `summarize`, the program rows of a build; `Finding` (field, code, message, `Tier`, operation, shape), `Findings`, `Built`, `Finished` and `ProgramSummary`. Moved from the CLI's `export.rs`. |
| `names.rs` | `program_names` and `program_suffixes`: `<stem>.nc`, `<stem>-<k>-T<n>.nc`, `<stem>-female-<k>-T<n>.nc`. |
| `summary.rs` | The export summary's lines: each operation and its depth, a carve's deepest cut and floor ridges, extents and edge extents, the cycle-time `Estimates` and `EstimateConfig`, board headings, what to do before a program starts. |
| `report.rs` | `export_report`: the lines `export` prints once its files are written (run order, operations, extents, estimates, pauses, totals), which the app's Export step shows as its result. Moved from the CLI's `export.rs`. |
| `preview.rs` | The toolpath preview SVG of `--preview` (`render`), a pure function of the project and the generated toolpaths, ported from PureCutCNC `src/engine/designPrint/svg.ts`; `marks`, the tab marks, start points and holes it draws, as geometry for the app's Viewport, read by the kind of cut: tab marks on a Profile and an Inlay's cut-out only, and on a carve a start point where the bit comes down into the work, not at every step deeper. |
| `project_from.rs` | `drawing`: the project `init` writes for a drawing, with or without the placeholder tool and Profile; `picture`, the one it writes for a picture, with a V-bit and a Photo V-Carve; `fit_stock`, the stock it sizes round some bounds at a margin, which the app's *Fit stock to shapes* reuses. |
| `picture.rs` | A decoded picture made a `Bitmap` at a size and a corner: `picture_into` (`image add`), `picture_rows` (`image list`). |
| `prep_report.rs` | The lines `image-prep` prints about a run, shared with the app. |
| `setup_sheet.rs` | The setup sheet: one HTML page of what an export wrote, program by program, from the export's own rows. |
| `import.rs` | `Drawing` (an SVG or DXF import) and `drawing_into`: a drawing's shapes appended to a project, placed as drawn, under fresh ids. |
| `checks.rs` | The artwork checks as both shells report them: `svg`, the checks `init` prints for an SVG and the app's Import drawing sheet lists; `project`, the two `check` reads from a project's shapes and the app's Warnings drawer lists; `drawing`, the same two for a DXF before it is imported. The rules and the words are `dc-import`'s. |
| `tabs.rs` | `place_request`, what `tabs place <dcv> <op>` does, and the `dc-cam` placement functions re-exported. |
| `error.rs` | `JobError`. |

### `dc-app` (`crates/dc-app/src/`)

| Module | Contents |
| --- | --- |
| `lib.rs` | The crate's surface: `App`, `Command`, `CommandError`, the history, selection, view, generation, strings, units and preferences types. |
| `app.rs` | `App`: the project history with its labels, `run`, `undo` and `redo` returning the step's label, `open`, `serialize` and `mark_saved` with `is_dirty`, the selection and view, `pending`, `accept` and `warnings`; `new_project`, the empty project *New* makes; `editing`, the operation the editor shows, apart from the shapes selected, and `toggle_hidden`, an operation's eye; `replaced`, which counts the projects opened so the shell tells a new project from an edit. |
| `command.rs` | `Command`, every change to the project: `apply` returns the next snapshot and never touches the caller's; `label` names the undo step from the string table. `command/instances.rs` holds the transforms about the selection's bounding-box centre, linked copies, deletion and stock fitting; `command/operations.rs` the operation list, tool removal and the field an edit changed; `command/bitmaps.rs` a picture's move, resize, flags and deletion. |
| `error.rs` | `CommandError`: a locked shape, an unknown id, a tool in use, a value that cannot be stored, a value that is not a length, and the lower crates' errors. |
| `history.rs` | `LabelledHistory`: `dc_project::History` with one label per past and future snapshot. |
| `selection.rs` | `Selection`: instances, tabs or pictures, each among its own kind; pruned when a snapshot drops what was selected. The operation the editor shows is `App::editing`, apart from it. |
| `view.rs` | `View`, `Camera`, `Visibility`, `ColourMode`: view state, never project state; `readout`, the cursor's position and the Z range the Viewport's bottom-left pill shows, and `has_toolpaths`, whether its toolpath controls show. |
| `generation.rs` | `Generation`, `SnapshotId`, `Job`, `JobResult`: which operations are pending or stale for the current snapshot, the results and `finish` bookkeeping; a result from an older snapshot is dropped. Each operation's `digest` hashes everything its generator reads, so an edit makes pending only the operations it affects. |
| `strings.rs` | `Strings` and the generated `StrKey` (`build.rs` from `strings/en.toml`): `get` and `fmt` with `{name}` placeholders. |
| `units.rs` | `format` and `format_rate` in the project's display units with the unit after the value; `parse_length_mm` and `parse_rate_mm_min` through `dc_project::units_input`, a bare number in the project's units. |
| `prefs.rs` | `Prefs`, `Theme`, `WindowGeometry`, `Panels`: the record behind `gui.toml`, whose file is `dreamcarve-shell`'s. |
| `tools.rs` | The Tools step: `ToolField`, `field_text`, `field_unit` and `set_field` (a feed, plunge or RPM shown empty while unset, never prefilled), `unset_rates` with the file's message, `users` for *Remove*'s refusal, `unused_hint`, `add_from_library` (`tools add`'s copies), `duplicate`, `save_to_library`. The editor's rows per type (`fields_for`, `rows`: a tapered ball nose's tip diameter, taper per side and flute length), every value a tool still needs from `Tool::unset_values` (`unset_values`, `required`, `lacks_value`), and a row's labelled angle and second line (`angle_text`, `row_detail`). |
| `library.rs` | The Tool Library window's model, ported from PureCutCNC `src/components/cam/toolLibraryDialogModel.ts` with its tests: `Filter` and `KindFilter`, search over name, vendor and diameter, the "in project" marking, the ticked rows and `build_add`, and `import` with its report. The table's `Column`s and each row's `Cell`s (`row`: the cutting diameter, the tip, the angle labelled), and the import review's `review_status`, `review_unset` and `skipped`. |
| `operations.rs` | The Operations list: `Kind` (the eight kinds), `add_menu` (the kinds the selection can take, then the rest with the reason), `new_operation` (the selection, the first fitting tool, `<Kind> <n>`, the CLI's defaults with every depth `null`), `add`, `reorder`, and `rows` with each row's detail line and status cell. |
| `editor/` | The operation editor. `fields.rs`, ported from PureCutCNC `src/components/cam/operationFields.ts`: `Group` in capability 66's order, `FieldKind` (one shell renderer each), `Row` (group, key path, kind, label, help, tool default, rule) and each kind's `table`; `rows`, `groups`, and `row_for`, which matches a located finding to its row by key path. `set.rs`: `record`, `get` and `set` by key path through `serde_json::Value`, read back by the file's deserializer. `targets.rs`: the Targets group's add, replace and remove. `mod.rs`: each row's text, tool-derived placeholder, choice, switch, reset and located message, and the `UpdateOperation` each edit is; the tools a picker offers (`offered_tools`, which asks validation what a carve may cut with), the line under it (`tool_note`) and an Inlay's derived lines, its seat line among them (`seat_line`). |
| `import.rs` | The Import drawing sheet's model: `DrawingImport` over the bytes the shell read, re-read through `dc-import` whenever an option changes (DXF units, required when the file declares none; join tolerance; ticked layers), with the summary, the layer list, the importer's refusal that disables *Import*, its warnings, the preview (the drawing alone, as drawn, with no stock outline) and the `ImportDrawing` command. `App::import_drawing` runs it and keeps the warnings as the import's notes; `App::new_from_drawing` is `project_from::drawing` without defaults. `PictureImport` is the Add picture sheet's model the same way: the decoded picture, the one side typed and the other following, and the conversion's note. |
| `export.rs` | The export plan (#547): `plan` waits while any operation is pending or stale, then builds the snapshot afresh with `dc_job::build_board` (S20); `Options` (program split override, board ticks, date), `Ready` with the rows, findings, preview, file names under a stem and the CLI's report; `total_time` and `operation_times`, the estimates outside the Export step; `several_tools`, whether the program split applies. |
| `editor/clearing.rs`, `editor/summary.rs` | A carve's clearing-pass list as one block (add, delete, reorder, each one undo step), and each closed editor group's one-line summary, built from the same rows the fields draw. |
| `project.rs` | The project's settings as the Stock and Export steps show them: stock, origin, heights and post-processor fields, the command each typed value becomes and the message located on it; `Group`, the Export step's three groups of output settings, each with a one-line summary of its values. |
| `tree.rs`, `properties.rs`, `arrange.rs`, `mixed.rs` | The Design Tree's rows; the Properties panel's placement fields; the Arrange actions (rotate, flip, centre); the value a multi-selection shows, or *Mixed*. |
| `pictures.rs` | A picture selected and placed: the Properties of the selected pictures (their footprint as a shape's X, Y, W and H, the pixel size, the Photo V-Carves that carve them), the move a typed value, a drag or a nudge is, and the resize one picture's typed W or H is, its proportions kept. |
| `scene.rs`, `viewport/` | The Scene, the read model the Viewport draws, rebuilt per snapshot or result, never per frame; the camera, picking, grid, transform handles, direction arrows and tab marks as pure functions on it. |
| `warnings.rs`, `status.rs` | The Warnings drawer's rows, each the line `dreamcarve check` prints; the status bar's cells as data. |
| `onboarding.rs` | The start screen's example thumbnails, drawn from the examples' own shapes. |
| `steps.rs` | The five steps (Design, Stock, Tools, Operations, Export): each step's `State` from the project and its findings (`states`), the step a finding belongs to (`step_of`), what *Continue* opens (`next`), the step a project opens on (`opening`), each step's summary line, and `counts`, the problems the status bar and the rail's Warnings button count (none on a blank project). |
| `test_support.rs` | `Harness`: an `App` from fixture bytes, commands run, pending jobs executed inline with the real `dc_job` functions (`test-support` feature). |

### `dreamcarve-app` (`crates/dreamcarve-app/src/`)

| Module | Contents |
| --- | --- |
| `main.rs` | The binary: `--open <path>` or a bare path, the config directory, and the `Shell` in an eframe window on wgpu. On Windows, a GUI program with no console. |
| `lib.rs` | `Shell`, a library too so the window tests (`tests/kittest.rs` and the others in `tests/`) build it in egui's harness. |
| `shell/` | The window's frame: the shell itself, the header with its menus, the rail and the status bar, focus, file dialogs and files, the toolpath view, the theme, and the worker thread that runs generation off the UI thread. |
| `panels/` | The step panel and the Inspector (`side.rs`) and what they hold: the Design Tree, Properties and the Arrange group, the Stock step, the Export step (its readiness card, the findings that block export, the output settings, the plan and the result), the Tools and Operations lists, the tool editor, the operation editor and its clearing list; the start screen; the Tool Library, the Import drawing and Add picture sheets, the Warnings drawer and the unsaved-changes sheet. |
| `viewport/` | The Viewport: the stage and stock sheet, the wgpu line layers (`renderer.rs`), level of detail, the floating controls (`controls.rs`: the tools, Fit and the zoom, the colour mode and Layers, the read-out, and the room Fit leaves for them), the legend, the pictures as textured quads (`pictures.rs`), the tab tool, and the overlay egui's painter draws. |
| `widgets/` | The app's own controls: icons and the operation glyphs, buttons, the field with a unit, rows, the segmented control, the switch, the checkbox, chips, cards, the disclosure section, the fading scroll area, the spinner. |
| `keyboard.rs` | The shortcut table, every shortcut once with its menu row; the Keyboard chapter's table is tested against it. |

### `dreamcarve-shell` (`crates/dreamcarve-shell/src/`)

| Module | Contents |
| --- | --- |
| `staged.rs` | `StagedFile` and `commit_all`: every file of an export written to a temporary name beside its destination and renamed into place at the end, or none (#71); `is_preview`, the check before a preview is written over an existing file. Moved from the CLI's `cli.rs` and `export.rs`. |
| `clock.rs` | UTC dates from the system clock, with no date crate: `today_utc` for a program header, which the CLI and the app's export both write; the CLI's `build.rs` includes it for `--version`. Moved from the CLI. |
| `config_dir.rs` | `Paths`: the user config directory (`DREAMCARVE_CONFIG_DIR` or the platform's) and the files in it. |
| `config.rs` | `config.toml` (`[export]`, `[estimate]`): strict, a typo is an error at its line and column. |
| `gui_prefs.rs` | `gui.toml`, the app's preferences: layout, visibility, recent files, theme. Lenient: a missing file is the defaults, an unknown key a note. |
| `library.rs` | `tools.json`: `read` (a missing file is `None`), `write` (staged, creating the directory) and `export`. Moved from the CLI's `tools.rs`. |
| `error.rs` | `ShellError`, whose messages are the CLI's. |

### `dreamcarve` (`crates/dreamcarve/src/`)

| Module | Contents |
| --- | --- |
| `main.rs` | Entry point: runs the command and maps its result to an exit code. |
| `cli.rs` | Arguments (clap), exit codes, the date. |
| `output.rs` | `outln!`, `out!`, `errln!`: all printing; a closed stdout pipe stops output and keeps the exit code (#216). |
| `init.rs` | `init`: SVG, DXF or picture import, printed with the import checks' warnings; the DXF-only `--units`, `--join-tolerance` and `--layer`, and a picture's `--width` or `--height`; the project itself is `dc_job::project_from`'s. |
| `image.rs` | `image add` and `image list`: a project's pictures. |
| `prep.rs` | `image-prep`: reads one SVG or DXF, writes one SVG, never over an existing file, and prints the report. |
| `export.rs` | `export`: `--post`, `--one-program`, `--programs-per-tool`, `--preview`, `--board`; the output paths, the staged write and the printed summary of what `dc_job::build` produced. |
| `check.rs` | `check`, and its `--preview`. |
| `config.rs` | `config path`, and the user config as the commands read it, through `dreamcarve-shell`. |
| `tools.rs` | `tools list`, `add`, `import`, `export`, `remove`: `tools.json` through `dreamcarve-shell`, the report from `dc_library::import`, and for `add` the project. |
| `tabs.rs` | `tabs place`, `list`, `clear`: the project's placed tabs. |

Behaviour tests for the binary are in `crates/dreamcarve/tests/`: `cli.rs`
for every command, with committed programs in `tests/expected/`, and
`quick_start.rs`, which runs the commands of the quick start chapters,
read from the chapters themselves. `crates/dc-job/tests/` holds the
programs, previews and projects the CLI wrote before the pipeline moved
(`expected/`), which the pipeline must still produce byte for byte.
`crates/dc-gcode/tests/` holds the golden comparisons (`golden.rs`), the
property tests (`properties/`) and the V-Carve and Inlay stress sweep
(`stress/`, `--ignored`), which generates difficult designs and checks
every program against S1–S19 and that each plug seats. The app core's
tests are in `crates/dc-app/tests/`, a file for each module, run
headless. The app's window tests are in `crates/dreamcarve-app/tests/`:
`kittest.rs` and `library.rs` drive the window in egui's harness,
`book.rs` writes the book's screenshots and `review.rs` the review's
(both `--ignored`: they write files and need a GPU adapter).

## Outside the workspace crates

- `tools/check-headers`: the license header checker CI runs.
- `tools/doc-figures`: draws the user guide's geometry figures from real
  engine output (`cargo run -p doc-figures`); a test fails when a committed
  figure is stale.
- `tools/third-party-licenses`: gathers the license texts shipped with
  release binaries.
- `tools/gcode-conformance`: runs the LinuxCNC and generic programs
  through LinuxCNC's `rs274` interpreter, in the `gcode-conformance` CI
  job. See [Verifying a post-processor](post-processor-checklist.md#linuxcncs-interpreter).
- `tools/rs274-motion`: runs the LinuxCNC, Mach3, UCCNC and generic
  programs through LinuxCNC's `rs274` interpreter, and compares its
  motion with `dc-gcode`'s reading. Needs `rs274` or a container; see
  [Verifying a post-processor](post-processor-checklist.md).
- `tools/grbl-sim-motion`: runs the grblHAL and GRBL programs through the
  grblHAL Simulator and grbl-sim and compares their motion with
  `dc-gcode`'s reading. Needs the simulators or a container.
- `tools/linuxcnc-sim`: runs programs through LinuxCNC's full simulation,
  headless. Needs LinuxCNC or a container.
- `tools/parity/`: Node scripts, not a Cargo crate, that drive a local
  PureCutCNC checkout to capture golden outputs and draw the synthetic
  fixtures; `tools/parity/photo/capture.py` runs RasterCarve's command
  line for the Photo V-Carve goldens. Not needed to build or test
  DreamCarve. See [Golden corpus](golden-corpus.md).
- `tests/golden/`: the captured PureCutCNC and RasterCarve outputs, with
  each case's DreamCarve project, that `cargo test` compares against.
- `fixtures/`: input files the user docs link to, such as `coaster.svg`.
- `tests/fixtures/`: input files for tests and the quick starts: the
  plaque in SVG and DXF, the sign, the "DC" inlay, line art and other
  image-prep drawings (`prep/`), the photo pictures (`photo/`), the V-Carve
  text polygons (`vcarve/`), DXF entity cases, and hand-written Fusion
  libraries.
- `docs/book/`: this site.

The design spec of each phase is the authority on where new code goes.
Rules for what may live in each crate are in `docs/rust-guidelines.md`,
"Crate boundaries".
