# DreamCarve Roadmap

This is the durable plan: what 1.0 means, the phases that get there, and the
rules every phase follows. Per-phase detail lives in `docs/specs/phase-N/`.
Decisions that were hard to reverse live in `docs/adr/`. Vocabulary lives in
`CONTEXT.md`. Work items live in GitHub Issues.

## What 1.0 means

A hobbyist can take a part from sketch or import to verified G-code for a
3-axis router without leaving DreamCarve, on Windows, macOS, or Linux, and trust
the output enough to run it. Feature scope at 1.0 matches the PureCutCNC
reference implementation except where this plan says otherwise.

## Principles

1. **Every phase ships a working product.** Someone can cut something with the
   output of each phase. Phases end with a tagged release, binaries for all
   three desktop platforms, user documentation, and a quick start.
2. **Spec first.** Each phase begins with a functional spec (what the user can
   do, how we know it works) and a design spec (how it is built, what is
   tested). Both are reviewed in a PR before implementation issues open.
3. **Test first.** Every implementation slice is a GitHub issue that lists its
   test cases before any implementation PR opens. The red test lands before the
   code that makes it green. See "How work flows" below.
4. **Port, don't invent.** PureCutCNC has solved most of these problems. Read
   its code first, port idiomatically, keep its unit tests as the red step, and
   use its output as a golden corpus. Attribution rules are in `AGENTS.md`.
5. **Machine output is safety-critical.** Never invent feeds or speeds. Never
   emit motion below material bottom. Every post-processor change has tests
   asserting safe Z, plunge depth, and feed rates on the emitted moves.

## Settled decisions

| Decision | Choice | Where recorded |
| --- | --- | --- |
| Internal coordinates | Y-up, Z-up, millimetres, machine-native | ADR-0001 |
| Project file | Own JSON format, `.dcv`, versioned with migrations | ADR-0002 |
| 3D view | Heightfield simulation only, no CSG | ADR-0003 |
| MVP shape | Headless: library crates + CLI. GUI arrives CAM-first (phase 4, imported geometry only), CAD second (phase 5) | this file |
| GUI toolkit | egui + wgpu | phase 4 design spec |
| Crate layout | Cargo workspace from day one | phase 0 design spec |
| 2D geometry | Polylines with arc segments. Béziers flattened at import to a project tolerance. Arc fitting at export | phase 1 design spec |
| Geometry crates | cavalier_contours (offsets), i_overlay (booleans), kurbo (curves) | phase 1 design spec |
| Feature model | Definition/instance split from day one; snapshot undo history in the model | phase 1 design spec |
| Tools and post-processors | Projects embed copies. App-level library in user config dir from phase 2 | phase 1 and 2 design specs |
| Controllers | grblHAL primary and validated on hardware. GRBL alongside in phase 1. LinuxCNC, Mach3, UCCNC, generic in phase 2 | phase 1 functional spec |
| Operation names | Profile, Pocket, Drill, V-Carve, Engrave; Surface Rough and Surface Finish from phase 7 | `CONTEXT.md` |
| Multi-tool export | One program with tool changes, or one program per tool. The user sets a default once; each export can override it | phase 2 functional spec |
| Toolpath coverage | Every carving toolpath common in hobby CAM is planned or explicitly excluded; see "Toolpath coverage" below | this file |
| Tool import | Fusion `.tools`/`.json` only, following Autodesk's published schema. Other vendor formats post-1.0 | phase 2 functional spec |
| Platforms | Windows, macOS, Linux desktop. No tablet, no phone, no web | this file |
| Localization | Strings keyed from the first GUI phase. English only until phase 8 | phase 4 design spec |
| Fonts | Bundled OFL fonts plus user font files. No system font enumeration | phase 5 design spec |
| STEP import | Post-1.0, pending a permissively licensed Rust reader | this file |
| Versioning | `0.N.0` tags the end of phase N; fixes between phases bump the patch. 1.0.0 is phase 9. Builds other than a release tag print `<version>-dev.<UTC build time>+g<hash>`; right after tagging, bump `Cargo.toml` to the next patch | this file |

## Toolpath coverage

The carving toolpaths common in hobby CAM, compared against VCarve Pro's
published toolpath list at the level of what each is for (laser toolpaths
excluded). Each one is either planned or deliberately left out. A toolpath
PureCutCNC lacks has nothing to port, so it would be designed from scratch;
those wait until after 1.0.

| Toolpath | Where | Note |
| --- | --- | --- |
| Profile | Phase 1 | Tabs in phase 1; lead-in/out and ramp entry in phase 2 |
| Pocket | Phase 2 | Offset and Raster clearance |
| Drill | Phase 2 | V-bit countersinking in phase 3 |
| Engrave | Phase 2 | Fixed-depth cut along the line. V-bit engraving whose width follows the shape is V-Carve |
| V-Carve | Phase 3 | With flat-bottom clearing |
| Surface Rough, Surface Finish | Phase 7 | From a mesh or a grayscale heightmap |
| Inlay | Post-1.0 | No PureCutCNC reference. Builds on V-Carve |
| Fluting | Post-1.0 | No PureCutCNC reference |
| Texture | Post-1.0 | No PureCutCNC reference |
| Prism carving | Post-1.0 | No PureCutCNC reference |
| Moulding | Post-1.0 | No PureCutCNC reference; also needs a swept 3D model, which the CAD side doesn't create |
| Quick (V-bit) engrave | Not planned | Covered by Engrave for fixed depth and V-Carve for V-bit work |

Toolpath management with no PureCutCNC reference (toolpath templates,
merging toolpaths, array-copying a toolpath, tiling oversize jobs) is also
post-1.0.

## Phases

Each phase lists its goal, what is in and out, the exit criteria, and the
PureCutCNC areas to read before writing its specs. "Real cut" means the user
cuts the phase's test part on a grblHAL machine and records the result in the
functional spec.

### Phase 0: Scaffold

**Goal:** a repository where the first TDD slice can land with every gate in
place.

**In:** Cargo workspace with the phase 1 crates (empty or near-empty), CI on
Windows/macOS/Linux running fmt, clippy, and tests, cargo-dist release config,
license-header check, mdBook docs site published to GitHub Pages, spec and
issue templates, PR template, parity-capture tool skeleton under
`tools/parity/`, `CONTEXT.md`, the first three ADRs.

**Out:** any CAM behaviour.

**Exit:** `cargo test --all` green on all three OSes in CI; docs site live with
the roadmap and a contributor guide; `v0.0.1` tagged and cargo-dist produces
binaries for a hello-world CLI.

**Read first:** `../purecutcnc/AGENTS.md` (coding rules), `scripts/check-license-headers.ts`,
`src/engine/toolpaths/parityCorpus.ts` and `parityRecord.ts`.

### Phase 1: Headless MVP

**Goal:** cut a real part from an SVG with one Profile operation.

**In:** `dreamcarve init` writes a `.dcv` from an SVG; `dreamcarve export`
writes grblHAL or GRBL G-code. Geometry crate with polylines, arcs, offsets,
flattening. Project model with stock, origin, feature definitions/instances,
tools, one operation, embedded post-processor, versioned JSON. Profile
generator: inside/outside/on-line, depth passes, holding tabs, safe Z, rapid
and plunge moves. Data-driven post-processor ported from PureCutCNC's
`MachineDefinition`. Export refuses when a tool lacks feed, plunge, or RPM.
Golden corpus from PureCutCNC for Profile cuts.

**Out:** every other operation, DXF, GUI, arc fitting of flattened curves,
lead-in/out, ramping, undo UI.

**Exit:** all Profile tests and safety tests green; the phase 1 test part cut
on grblHAL hardware; quick start "Your first cut from an SVG" published;
`v0.1.0` tagged.

**Real cut:** yes. Test part defined in the phase 1 functional spec.

**Read first:** `src/engine/toolpaths/edge.ts`, `tabs.ts`, `tabProfile.ts`,
`src/engine/gcode/postprocessor.ts`, `types.ts`, `definitions/grblhal.json`,
`src/import/svg.ts`, `src/types/project.ts` (`Tool`, `FeatureDefinition`,
`FeatureInstance`), `planning/G-code_Export_Design.md`.

### Phase 2: Full 2.5D from the command line

**Goal:** every 2.5D operation except V-Carve, from SVG or DXF, to any
supported controller.

**In:** Pocket with two clearance strategies, **Offset** (concentric inward
passes) and **Raster** (parallel lines at a user-set angle), each followed by
a wall finish pass, with climb/conventional direction and a user-set stepover
as a percentage of tool diameter. Rough and finish pockets. Drill (simple,
peck, dwell, chip-break, canned cycles where the post supports them), Engrave
(open and closed paths, at a fixed depth). Multiple operations per project
with ordering. Multi-tool export: one program with tool changes or one program
per tool, with a user default and a per-export override. Tool library in the user config directory with
import from Fusion `.tools`/`.json` and export to DreamCarve JSON. DXF import.
LinuxCNC, Mach3, UCCNC, and generic post-processors. Arc fitting at export.
Ramp and helical entry for pockets and profiles, and arc lead-in/out for
profiles (PureCutCNC `entry.ts`, `xyLead.ts`). Corner relief (dogbone, T-bone)
for pockets and profiles (`cornerRelief.ts`). Toolpath warnings as structured
codes. A `dreamcarve check` command that reports warnings without exporting.

**Out:** V-Carve, GUI, simulation, text.

**Exit:** parity with PureCutCNC's pocket/drill/carving test suites for the
ported behaviours; phase 2 test part cut; `v0.2.0`.

**Real cut:** yes (a pocketed, drilled, engraved plaque).

**Read first:** `src/engine/toolpaths/pocket.ts`, `pocketPatterns.ts`,
`drilling.ts`, `carving.ts`, `entry.ts`, `regions.ts`, `regionDomain.ts`,
`resolver.ts`, `src/engine/gcode/arcFitting.ts`, `src/import/dxf.ts`,
`src/machine/`, `planning/REGION_FEATURE_SEMANTICS.md`,
`planning/BAND_RESOLVER_SEMANTICS.md`.

### Phase 3: V-Carve

**Goal:** the headline operation, from the command line.

**In:** medial-axis V-carving of closed shapes with a V-bit, flat-depth
limiting with a clearing tool, offset-stepping V-carve mode, V-bit
countersinking on Drill. Carve-quality tests against the golden corpus.

**Out:** GUI.

**Exit:** parity with PureCutCNC's V-carve tests; phase 3 test part cut (a
lettered sign imported as SVG outlines); `v0.3.0`.

**Real cut:** yes.

**Read first:** `src/engine/toolpaths/vcarve.ts`, the `v_carve_medial`
operation kind in `src/types/project.ts`, related tests.

### Phase 4: GUI, CAM only

**Goal:** everything the CLI does, visually, on all three desktop platforms,
with imported geometry. No drawing tools.

The GUI arrives in two phases on purpose. This one is CAM: the user brings
geometry in from SVG (and DXF) and does all the machining work in the app.
Keeping drawing out lets the phase concentrate on the workflow that actually
produces G-code: stock, origin, tools, operations, preview, warnings, export.
Phase 5 adds CAD so the SVG step becomes optional.

**In:** egui + wgpu application. Open/save `.dcv`. Import SVG and DXF into
an open project. Design tree showing feature instances with visibility and
lock; select, move, rotate, scale, mirror, and delete instances (placement
only, no shape editing). Stock and origin setup. Tool library browser and
picker, including Fusion import. Create, edit, duplicate, delete, reorder,
and enable/disable operations of every kind from phases 1–3. 2D toolpath preview with direction
and depth colouring and per-operation visibility. Warnings panel. Export
dialog. Undo/redo wired to the snapshot history. Strings keyed for
localization.

**Out:** drawing or editing shape geometry, text, snapping, node editing, 3D.

**Exit:** a user can reproduce the phase 1–3 test parts from their SVGs
without the CLI; quick start "From SVG to G-code in the app"; `v0.4.0`.

**Read first:** `src/components/canvas/SketchCanvas.tsx` (navigation,
selection, and toolpath rendering paths only), `src/components/` CAM panel
and export dialog, `src/store/`, `src/app/toolpathGeneration/`,
`ARCHITECTURE.md` §2 and §11.

### Phase 5: GUI, CAD

**Goal:** VCarve-class drawing, so a part can be designed from nothing.

**In:** draw line, polyline, rectangle, rounded and chamfered rectangle,
circle, ellipse, polygon, regular polygon, **slot** (a first-class tool:
two centre points and a width, since slots are fundamental to CNC joinery and
fixturing), gear, arc, spline; text with bundled and user fonts; node editing with fillets; grid and
object snapping (endpoint, midpoint, centre, intersection); offset, array,
align, and distribute; join and trim; linked copies and Make Unique;
folders; construction geometry; bitmap backdrops for tracing; on-canvas
measurements; GUI tool-library editor. The spec may split this phase into
two releases (basic drawing, then advanced editing) if the slice list is
too long for one; the phase number stays 5 either way.

**Out:** 3D.

**Exit:** the phase 5 test part, designed entirely in the GUI with no import,
cut; quick start "Your first design"; `v0.5.0`.

**Real cut:** optional, since machine output does not change.

**Spec notes:** requirements captured ahead of the spec are in
`docs/specs/phase-5/NOTES.md`; fold them in when writing the spec.

**Read first:** `src/sketch/`, `src/text/`, `src/components/canvas/`
drawing modes, snapping, and edit-session code, `src/commands/creationShapes.ts`,
`src/store/helpers/featureDefinitions.ts`, `DRAWING_MODES_TUTORIAL_VOICEOVER.md`.

### Phase 6: 3D preview and simulation

**Goal:** see the result before cutting.

**In:** heightfield material-removal simulation driven by the generated
toolpaths, rendered in a wgpu panel, with playback and per-operation
stepping. Uncut-material and gouge highlighting.

**Exit:** the simulation of each earlier test part matches the physical cut
to within the tool-radius tolerance stated in the spec; `v0.6.0`.

**Read first:** `src/engine/simulation/`, `src/components/simulation/`.

### Phase 7: Mesh machining

**Goal:** 3D surface work on imported meshes.

**In:** STL and OBJ import with orientation and silhouette extraction;
**grayscale heightmap import** (PNG, JPEG, TIFF) where pixel brightness maps
to Z over a user-set footprint and depth range, so a lake-depth chart or a
terrain tile carves without a 3D model; Surface Rough and Surface Finish
(parallel, waterline, constant scallop) and PureCutCNC's surface cleanup
passes, driven by either source; retained
2.5D material around a model. Re-examine ADR-0003 here: if retained-material
math needs CSG, add a crate and amend the ADR.

**Out:** STEP.

**Exit:** parity with PureCutCNC surface tests; two phase 7 test parts cut
(a relief from a free STL, and a lake-bed from a grayscale depth image);
`v0.7.0`.

**Spec notes:** requirements captured ahead of the spec are in
`docs/specs/phase-7/NOTES.md`; fold them in when writing the spec.

**Real cut:** yes.

**Read first:** `src/engine/importedMesh.ts`, `src/engine/toolpaths/roughSurface.ts`,
`finishSurface*.ts` (especially `getCachedHeightMap` and `safeToolTipZAt` in
`finishSurfaceParallel.ts`, the internal height map a grayscale import can
feed directly), `retainedMaterial.ts`, `src/import/stl.ts`, `obj.ts`.

### Phase 8: Workshop extras

**Goal:** the long tail PureCutCNC ships.

**In:** sheet nesting, operation booklet PDF with machining time estimates,
post-processor converter CLI,
`.camj` import, German and Chinese localizations, design printing and SVG
export, CAM Plan assistance, clamps as obstacles, trochoidal strategies if not
already ported, tabs on open shapes.

**Exit:** each item has its own spec slice and quick-start section; `v0.8.0`.

**Read first:** `src/engine/nesting/`, `operationBooklet/`,
`src/postProcessorConverter/`, `src/import/camj.ts`, `src/i18n/`,
`src/engine/designPrint/`, `operations/camPlan/`.

### Phase 9: 1.0

**Goal:** trustworthy for strangers.

**In:** signed installers that do not warn on macOS and Windows, a first-run
path to a first toolpath without reading docs, every externally reported
defect fixed or answered, performance budget met on a ten-year-old laptop,
full user manual.

**Exit:** `v1.0.0`.

## Post-1.0 directions

Not rejected, just after 1.0: STEP import, Vectric/Carveco/MillMage/Carbide
Create tool-library import, rotary and 4th-axis, parametrics, cloud sync,
system font enumeration, an agent-facing API. The toolpaths and toolpath
management features marked post-1.0 in "Toolpath coverage": Inlay, Fluting,
Texture, Prism carving, Moulding, toolpath templates, merge, array copy, and
tiling.

## How work flows

1. **Phase spec PR.** Copy `docs/specs/TEMPLATE-functional.md` and
   `TEMPLATE-design.md` into `docs/specs/phase-N/`. Fill them. Open a PR.
   Merge when the user signs off.
2. **Slice issues.** Break the design spec into GitHub issues, each small
   enough for one PR. Each issue body has a **Tests** section listing the
   test names and what each asserts. An issue without a Tests section is not
   ready; label it `needs-info`. A complete one gets `ready-for-agent` or
   `ready-for-human`.
3. **Red.** The first commit of the implementation PR adds the tests from the
   issue. They fail. Ported PureCutCNC tests count as red tests and keep the
   attribution header.
4. **Green.** Implement until green. Run the full gate from `AGENTS.md`.
5. **Refactor and document.** Update rustdoc, the mdBook chapter, and
   `INDEX`-style module docs touched by the change. Update `CONTEXT.md` if a
   term changed.
6. **Review.** PR review checks the tests match the issue, the attribution
   headers are right, and safety assertions exist for any machine-output
   change.
7. **Phase exit.** Real cut where required, quick start published, tag.

## Parity corpus

`tools/parity/` holds a TypeScript script, run with Node against a local
PureCutCNC checkout, that loads fixture `.camj` projects, runs
`computeOperationToolpath` and `runPostProcessor`, and writes
`tests/golden/<case>/program.nc` and `toolpath.json`. The goldens are
committed; `cargo test` reads them and never needs Node. Because PureCutCNC
flips to Y-up at export, its G-code is directly comparable to DreamCarve's
without transformation. Toolpath JSON is compared after mapping Y.
Tolerances are per case and stated in the fixture's README.

## Documentation

`docs/book/` is an mdBook with two parts: **User Guide** (install, quick
starts, one chapter per operation, tool library, post-processors, safety) and
**Contributing** (architecture, crate map, how work flows, porting from
PureCutCNC, licensing). CI builds it on every PR and publishes `main` to GitHub
Pages. Rustdoc for the library crates is published alongside. Every phase adds
its quick start before tagging.
