# Phase 2: Full 2.5D from the command line — Design Spec

**Status:** draft
**Functional spec:** `functional.md`

## Overview

Phase 2 adds three generators to `dc-cam` (Pocket, Drill, Engrave), extends
the Profile generator (entry, leads, corner relief), and puts one shared
sequencing step between every generator and the post-processor. It adds a
DXF importer to `dc-import`, four post-processor definitions and arc
fitting to `dc-gcode`, and a new pure crate `dc-library` for the tool
library and the Fusion importer. The CLI gains `tools`, `config` and the
export flags, and reads the user config directory.

```
SVG / DXF ──dc-import──▶ FeatureDefinitions ──┐
                                              ├─▶ Project (.dcv v7, dc-project)
user edits .dcv ──────────────────────────────┘        │
tool library (dc-library) ─ tools add ─────────────────┘
                                                       ▼
   dc-cam::{profile, pocket, drill, engrave}::generate(project, op)
                         │  Vec<CutBlock> + Vec<Warning>
                         ▼
   dc-cam::sequence::sequence(blocks, order, links) ─▶ Toolpath (project coords, mm, Y-up)
                         │
                         ▼
   dc-cam::order::check(project, toolpaths) ─▶ cut_inside_freed_part, tool_change_avoidable
                         │
                         ▼
   dc-gcode::emit_programs(project, ops, mode) ─▶ arc fitting ─▶ Programs + warnings
                         │
                         ▼
   safety pass on the parsed Program (S1–S15; canned cycles expanded first)
```

Everything is millimetres, Y-up, Z-up, stock bottom at Z = 0 (ADR-0001).
The generators are pure; the CLI owns every file, clock and directory.
Where this spec names a PureCutCNC rule that depends on its Y-down frame,
the Y-up form is stated and the Y-down form is noted so the port can be
checked against upstream tests.

**Assumption on #106.** If "Post-processor as versioned reference plus
user settings" lands before the post-processor slice starts, the four new
definitions are added as `definitions/<id>/v1.json` and the new settings
(`use_canned_cycles`) go in the settings block. Otherwise they follow
phase 1's embedded layout. The emitter is the same either way.

## PureCutCNC sources

| DreamCarve | PureCutCNC | Port notes |
| --- | --- | --- |
| `dc-cam::pocket` | `src/engine/toolpaths/pocket.ts` (`buildInsetRegions`, `buildOffsetRegionTree`, `cutOffsetRegionNode` inner-first, `buildPocketParallelSegments`, `transitionToCutEntry`, `generateStepLevels`, `orderClosedContoursGreedy`, `rotateContourToNearestEntry`), `pocketPatterns.ts` (`OPERATION_PATTERN_SUPPORT`, `areaCoverage`) | Only `offset` and `parallel` (named `raster`). No seeded offset, trochoidal, slot feed, engagement feed, corner rounding, wall cleanup, model sections, clamps. Clipper integer paths become `dc-geometry` offsets with cavalier_contours and booleans with i_overlay. Direction rule re-derived (see Pocket algorithm). `pocketAngle` sign re-derived. Islands come from `nest`, not the band resolver. |
| `dc-cam::pocket::rest` | `src/engine/toolpaths/restRegions.ts`, `finishFloorCoverage.ts` (`regionAreaPaths`, `floorCoverage`, `restrictFloorRoot` noise rule) | Rest region = region minus the earlier tool's reachable area. The coverage test is ported; the island-face floor restriction is not (no multi-depth instances in phase 2). |
| `dc-cam::drill` | `src/engine/toolpaths/drilling.ts` (`generateDrillingToolpath`, `emitDrillCycle`, `sortTargetsByNearestNeighbor`, `DrillCycle`), `drillingRetract.test.ts`, `camOperationSmoke.test.ts` drill cases, `toolpaths.test.ts` drill ordering | Circle detection on polylines, not a `circle` feature kind. Chip-break gap is a mm field, not a constant in project units. Dwell is emitted as `G4` when expanded (upstream loses it). No helical bore, no countersink (phase 3), no clamps. Nearest neighbour starts from the first target in drawing order (D5), not the origin. |
| `dc-cam::engrave` | `src/engine/toolpaths/carving.ts` (`generateFollowLineToolpath`, `buildCarveLevels`), `carving.test.ts` direct-mode cases | Direct mode only; no trochoidal carve, no region masks. Depth is from the stock top and clamped by S1, not to absolute 0. Direction alternation added (D9). |
| `dc-cam::entry` | `src/engine/toolpaths/entry.ts` (`synthesizeEntry`, `findHelixPlacement`, `findLargestClearanceCircle`, `findRampPlacement`, `emitHelix`, `emitRamp`, `pitchFromRampAngle`, `plungeLimitedFeedScale`, `helixAngularDirection`), `entry.test.ts` | Helix sense re-derived for Y-up. Search caps kept (`MAX_HELIX_RADIUS_ATTEMPTS`, 72 angles, 12 endpoint candidates, 20 000 cells). Profile ramp (along the path) is DreamCarve's; PureCutCNC's zigzag ramp serves Pocket. |
| `dc-cam::lead` | `src/engine/toolpaths/xyLead.ts` (`planXyLeadIn`, `planXyLeadOut`, `contourArrivals` corner scoring, `validateLead`, radius and sweep ladders), `xyLead.test.ts`, `xyLeadEdge.test.ts` | The user sets the radius (VCarve); the ladder shrinks from it. Sweep ladder kept. Overcut is DreamCarve's (VCarve). Feed is the cut feed, not the plunge-scaled feed, because the descent is finished before the lead. |
| `dc-cam::relief` | `src/engine/toolpaths/cornerRelief.ts` (`collectReliefCorners`, `reliefExcursionEnd`, `generateCornerReliefPass`, `chooseTBoneEdge`, `mainPathCutsAt`), `cornerRelief.test.ts` | `t_bone` goes on the longer edge (VCarve placement); `longest_edge` variant dropped. Keep-outs dropped. |
| `dc-cam::sequence` | `src/engine/toolpaths/multiFeature.ts` (`perFeatureOperations`, `mergeToolpathResults` nearest), `geometry.ts` (`greedyNearestNeighbor`), `pocket.ts` (`transitionToCutEntry`, `transitionLinksAtDepth`) | New module per D10. Blocks, containment precedence, nearest neighbour, links. `maxLinkDistance = tool diameter` for stay-down links inside cleared material. |
| `dc-cam::order` | `src/engine/operations/camPlan/createCamPlan.ts` (outside-last as a planning rule) | DreamCarve warns instead of planning: `cut_inside_freed_part`, `tool_change_avoidable`. |
| `dc-gcode::arcfit` | `src/engine/gcode/arcFitting.ts` (`fitArcsInMachineMoves`, `splitArc`, `resolveEmittedArc`, `runSurvivesFormatting`, `applyEmittedArcFallback`), `arcReconstruction.ts` (`findArcRunsInPoints`, Kasa fit, endpoint projection, gates), `arcFitting.test.ts` | Runs in project space before the machine transform (no mirror, so handedness is preserved). Tolerance from the project. S12 gates added to the safety pass. |
| `dc-gcode::emit` canned cycles | `src/engine/gcode/postprocessor.ts` lines 301–430 | One cycle word per Drill operation; mixed cycles expand. `G4` dwell on expansion. Safety pass expands cycles (S11). |
| `dc-gcode/definitions/{linuxcnc,mach3,uccnc,generic}.json` | `src/engine/gcode/definitions/*.json` | Data start point only. Every line re-verified against the controller's documentation; `G43 H` lines removed; `G40 G49 G80` kept where the controller documents them; see "Post-processor definitions". |
| `dc-import::dxf` | `src/import/dxf.ts` (entity set, `$INSUNITS` table, bulge-to-arc, INSERT transform and skip rules, stitching and closing, dedupe) | Parsing delegated to the `dxf` crate. No Y flip (DXF is Y-up). `ELLIPSE` added. Join tolerance 0.01 mm. Entity skips are warned with counts. |
| `dc-library::fusion` | none (PureCutCNC has no Fusion tool import; `src/toolLibrary.ts` is a bundled list) | New. Written against Autodesk's statements and vendor-published `.tools` files; see "Fusion import". |
| `dc-library::library` | `src/toolLibrary.ts` (`parseToolLibraryFile` validation rules) | Validation rules ported (positive diameter, flutes ≥ 1, stepover in (0, 1], V-bit angle in (0, 180)). Persistence is DreamCarve's. |
| `tests/golden/{pocket-*, drill-*, engrave-*, profile-{ramp,lead,dogbone}, order-*}` | captured via `tools/parity/` | See Test plan. Raster angle and helix sense are mirrored in the `.camj`; each case's notes say how. |

Files in the first column that port logic carry the upstream attribution
header.

## Data model

### dc-project (format version 7)

```rust
pub const CURRENT_VERSION: u32 = 7;

pub struct Project {
    // phase 1 fields unchanged, plus:
    /// Fit arcs to straight runs at export within this deviation. `None`
    /// turns fitting off. `init` writes 0.01.
    pub arc_fit_tolerance_mm: Option<f64>,
}

pub struct Tool {
    // phase 1 fields unchanged; `step_over_fraction` now read by Pocket.
    /// Tool number for `T<n>` in programs and file names. `None` means
    /// the tool's position in `Project::tools`. Migration: null.
    pub number: Option<u32>,
}

pub enum ToolKind {
    EndMill, BallNose, VBit { included_angle_deg: f64 }, Engraver,
    /// Twist drill. Phase 2 drills with any cutter; the kind drives
    /// warnings and Fusion import only.
    Drill,
}

pub enum Operation { Profile(ProfileOp), Pocket(PocketOp), Drill(DrillOp), Engrave(EngraveOp) }

pub enum MachiningOrder { FullDepthFirst, PassFirst }   // "full_depth_first" | "pass_first"

/// How the tool reaches each pass depth. Saved as a string for `Plunge`,
/// a one-key object otherwise.
pub enum Entry {
    Plunge,
    /// Pocket: zigzag inside the region. Profile: along the cut path.
    Ramp { angle_deg: f64 },                         // init: 15.0; valid (0, 45]
    /// Pocket and Profile (Inside, Outside): a helix in removed material.
    Helix { diameter_fraction: f64, angle_deg: f64 }, // init: 0.8, 5.0; fraction in (0, 1]
}

pub struct Lead {
    pub radius_mm: f64,          // > 0
    pub sweep_deg: f64,          // init 90; valid (0, 180]
    pub lead_out: bool,          // init true
    pub overcut_mm: f64,         // init 0; ≥ 0
}

pub enum CornerRelief { None, Dogbone, TBone }       // "none" | "dogbone" | "t_bone"

pub struct ProfileOp {
    // phase 1 fields unchanged, plus:
    pub machining_order: MachiningOrder,  // migration: full_depth_first
    pub entry: Entry,                     // migration: plunge; init: ramp 15°
    pub lead: Option<Lead>,               // migration: null
    pub corner_relief: CornerRelief,      // migration: none
}

pub enum ClearanceStrategy { Offset, Raster { angle_deg: f64 } }  // "offset" | { "raster": { "angle_deg": 45.0 } }
pub enum WallPass { Last, First, None }

pub struct PocketOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>, pub tool: ToolId,
    pub strategy: ClearanceStrategy,
    pub wall_pass: WallPass,                  // init: last
    pub depth_mm: Option<f64>,                // from stock top; None until set
    pub step_down_mm: Option<f64>,            // overrides tool.step_down_mm
    pub step_over_fraction: Option<f64>,      // overrides tool.step_over_fraction; (0, 1]
    pub direction: CutDirection,
    pub allowance_mm: f64,                    // init 0; ≥ 0 leaves material on walls (VCarve sign)
    pub rest_of: Option<OperationId>,         // an earlier enabled Pocket with the same targets
    pub entry: Entry,
    pub corner_relief: CornerRelief,
    pub machining_order: MachiningOrder,
    pub through_cut_allowance_mm: f64,
    pub safe_z_mm: f64,
}

pub enum PeckRetract { Full, ChipBreak { gap_mm: f64 } }
pub enum DrillCycle {
    Simple,
    Peck { peck_depth_mm: Option<f64>, retract: PeckRetract },  // None: tool step-down
}

pub struct DrillOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>, pub tool: ToolId,
    pub cycle: DrillCycle,
    pub dwell_s: f64,                         // 0 = none; combinable with any cycle
    pub depth_mm: Option<f64>,
    pub retract_mm: f64,                      // above the hole top; init 1.0; ≥ 0
    pub through_cut_allowance_mm: f64,
    pub safe_z_mm: f64,
}

pub struct EngraveOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>, pub tool: ToolId,
    pub depth_mm: Option<f64>,
    pub step_down_mm: Option<f64>,
    pub alternate_direction: bool,            // init false
    pub machining_order: MachiningOrder,
    pub through_cut_allowance_mm: f64,
    pub safe_z_mm: f64,
}
```

`Operation::{id, name, enabled, tool, targets, safe_z_mm, depth_mm}` are
accessors over every variant. `PostProcessor` gains one user setting:

```rust
pub struct CannedCycles { /* phase 1 fields */ }
pub struct ToolChange { /* phase 1 fields */ }
/// New top-level field on `PostProcessor` (or on the settings block after #106):
pub use_canned_cycles: bool,   // migration: false
```

**Migration v6 → v7** (`migrate::v6_to_v7`): add `arc_fit_tolerance_mm:
0.01` to the project (fitting on; a user who wants the phase 1 bytes sets
`null`), `machining_order: "full_depth_first"`, `entry: "plunge"`, `lead:
null`, `corner_relief: "none"` to every Profile, and `use_canned_cycles:
false` to the post-processor. A fixture at `tests/fixtures/dcv/v6/` loads
and, with `arc_fit_tolerance_mm` set back to `null`, exports byte-identical
G-code (acceptance 37). Open question 8 in the functional spec records why
this is a migration and not a serde default.

**Tool numbers.** `Tool::number` set → `T<number>` in every program and
in `<stem>-<k>-T<n>` file names; unset → the tool's 1-based position, as
in phase 1. Validation errors (`Problem::DuplicateToolNumber`, located at
the second tool) when two tools used by enabled operations resolve to the
same number, whether set or positional, and when a set number is 0.

**Validation** (`validate.rs`) adds, as errors: a `rest_of` that is not an
earlier enabled Pocket with the same target set; `step_over_fraction`
outside (0, 1] on the tool or operation; a Pocket or Engrave with no
step-down on operation or tool; a Drill whose `peck_depth_mm` is unset
when the tool has no step-down; a `Lead` radius ≤ 0 or sweep outside
(0, 180]; an `Entry::Ramp` angle outside (0, 45]; a `Helix` fraction
outside (0, 1]; `Entry::Helix` on a `Side::OnLine` Profile
(`Problem::HelixNeedsWasteSide`); `use_canned_cycles: true` on a
definition whose `canned_cycles` is `null`; `tool_change.mode: "m6"`
without an `M6` line (phase 1). Warnings: a Drill whose tool is not
`ToolKind::Drill` or `EndMill` (`Problem::DrillCutterUnusual`); a Pocket
or Profile with `corner_relief` on a tool whose step-down is unset
(`Problem::ReliefNeedsStepDown`, the relief pass is skipped); an Engrave
with a Side-less V-bit deeper than the bit's cone height
(`Problem::EngraveDeeperThanCone`).

### dc-cam

```rust
/// One contour, open path, drill point or relief notch at one pass depth,
/// as the generator produced it, before ordering and linking.
pub struct CutBlock {
    pub shape: ShapeRef,             // (instance, profile index) the block belongs to
    pub pass: usize,                 // 0-based pass index, top first
    pub z_mm: f64,                   // pass Z in project space
    pub entry: Point,                // XY where the descent ends and cutting begins
    pub descent: Vec<Move>,          // ramp/helix/plunge moves from `descent_from` to entry; empty = plunge
    pub descent_from_z_mm: f64,      // Z the descent may start at (safe Z, or the previous pass Z of this shape)
    pub cuts: Vec<Move>,             // Linear/Arc moves at z_mm (tabs may raise within)
    pub exit: Point,                 // XY of the last cut
    pub frees: bool,                 // this block's end frees material (final pass reaching the floor)
    pub cleared: Option<Region>,     // area this block has cleared at z_mm (pockets); None for profiles
    pub group: usize,                // blocks that must stay in generator order share a group (raster lines of one pass, relief notches)
}

pub struct SequenceInput<'a> {
    pub blocks: Vec<CutBlock>,
    pub order: MachiningOrder,
    pub containment: &'a [(ShapeRef, ShapeRef)],   // (inner, encloser) pairs from `nest` depth
    pub safe_z_mm: f64,
    pub tool_diameter_mm: f64,
    pub feed_mm_min: f64, pub plunge_mm_min: f64,
}
pub fn sequence(input: SequenceInput) -> Toolpath;

pub fn generate_profile(project: &Project, op: &ProfileOp) -> Result<(Toolpath, Vec<Warning>), CamError>;
pub fn generate_pocket(project: &Project, op: &PocketOp) -> Result<(Toolpath, Vec<Warning>), CamError>;
pub fn generate_drill(project: &Project, op: &DrillOp) -> Result<(DrillToolpath, Vec<Warning>), CamError>;
pub fn generate_engrave(project: &Project, op: &EngraveOp) -> Result<(Toolpath, Vec<Warning>), CamError>;
pub fn generate(project: &Project, op: &Operation) -> Result<(Generated, Vec<Warning>), CamError>;

/// Drill output: the toolpath plus the cycle records the emitter may
/// write as canned cycles. The toolpath is the authority (S11).
pub struct DrillToolpath { pub toolpath: Toolpath, pub holes: Vec<DrillHole> }
pub struct DrillHole { pub centre: Point, pub top_z_mm: f64, pub bottom_z_mm: f64, pub retract_z_mm: f64, pub cycle: CannedKind, pub peck_mm: Option<f64>, pub dwell_s: Option<f64> }
pub enum CannedKind { G81, G82, G83, G73, Expanded }

pub fn footprint(toolpath: &Toolpath, radius_mm: f64) -> Vec<Region>;   // swept area of cutting moves (S9, coverage tests)

pub mod order {
    pub fn check(project: &Project, generated: &[(&Operation, &Toolpath)]) -> Vec<Warning>;  // cut_inside_freed_part, tool_change_avoidable
}
```

New `WarningCode`s (params in parentheses): `EntryFallback (op, instance,
profile, requested, used)`, `HelixDiameterReduced (op, instance,
requested_mm, used_mm)`, `LeadShrunk (op, instance, requested_mm,
used_mm)`, `LeadDropped (op, instance, reason)`, `ReliefCornerTooTight
(op, instance, x_mm, y_mm)`, `ReliefCornerNotCut (…)`, `ReliefSkipped
(op, reason)`, `PocketNoRoom (op, instance, pass)` (tool wider than the
region at that pass), `PocketIslandTooClose (op, instance, island,
gap_mm)`, `DrillTargetNotCircle (op, instance, profile)`,
`DrillToolWiderThanHole (op, instance, hole_mm, tool_mm)`,
`DrillPeckDeeperThanHole (op, instance)`, `EngravePathDegenerate (op,
instance, profile)`, `CutInsideFreedPart (op, earlier_op, instance)`,
`ToolChangeAvoidable (tool, ops)`, `RestOfClearsNothing (op, rest_of)`.
All phase 1 codes stay.

### dc-gcode

```rust
pub enum ProgramSplit { ProjectDefault, OneProgram, ProgramPerTool }   // CLI flags
pub fn emit_programs(project: &Project, ops: &[EmitOp<'_>], split: ProgramSplit) -> Result<(Vec<Program>, Vec<Warning>), ExportError>;
pub struct EmitOp<'a> { pub op: &'a Operation, pub tool: &'a Tool, pub toolpath: &'a Toolpath, pub holes: Option<&'a [DrillHole]> }

pub mod arcfit {
    pub const ARC_FIT_MAX_SWEEP_DEG: f64 = 90.0;
    pub const ARC_RADIUS_CONSISTENCY_MM: f64 = 0.005;   // GRBL's check, grbl/gcode.c
    pub const ARC_RADIUS_GROSS_MM: f64 = 0.5;
    pub const ARC_RADIUS_RELATIVE: f64 = 0.001;
    pub fn fit(moves: &[Move], tolerance_mm: f64) -> Vec<Fitted>;   // Fitted::{Arc{..., fallback: Vec<Move>}, Line(Move)}
}
```

`ExportError` adds `OneProgramNeedsM6 { post }`, `CannedCycleUnsupported
{ post, cycle }` (only when `use_canned_cycles` is on and the definition
lacks that cycle's command: the emitter then expands, so this is a
warning `canned_cycle_expanded`, not an error), `EntryTooSteep { op, line
}` (S10), `OffMaterial { op, line }` (S9 on the parsed program), and
`ArcFitViolation { op, line }` (S12).

### dc-library

```rust
pub const LIBRARY_VERSION: u32 = 1;
pub struct Library { pub version: u32, pub tools: Vec<LibraryTool> }
pub struct LibraryTool { pub id: LibraryToolId, pub tool: Tool /* id ignored */, pub source: Option<String> }  // source: "fusion:<guid>:<preset>" or None
pub fn load(bytes: &[u8]) -> Result<Library, LibraryError>;
pub fn save(lib: &Library) -> String;                       // stable key order, one tool per block
pub fn add(lib: &mut Library, tool: Tool, source: Option<String>) -> AddOutcome;   // Added | AlreadyPresent (same fields)
pub fn find(lib: &Library, key: &str) -> Result<&LibraryTool, LibraryError>;  // by id, else unique name
pub mod fusion {
    pub fn import(bytes: &[u8]) -> Result<FusionImport, LibraryError>;    // .tools (zip) or .json
    pub struct FusionImport { pub tools: Vec<(Tool, String)>, pub skipped: Vec<Skipped> }
    pub struct Skipped { pub description: String, pub reason: SkipReason }
}
```

### User config (CLI only)

`<config dir>/dreamcarve/config.toml`:

```toml
[export]
tool_change_mode = "separate_programs"   # or "m6"; applied by `init` and `--post`
```

`<config dir>/dreamcarve/tools.json` is the library. `<config dir>` is
`directories::ProjectDirs::from("", "", "dreamcarve").config_dir()`:
`~/Library/Application Support/dreamcarve` on macOS, `%APPDATA%\dreamcarve`
on Windows, `~/.config/dreamcarve` on Linux. `DREAMCARVE_CONFIG_DIR`
overrides it (tests use this). A missing config file means the defaults; a
malformed one is an error naming the file and line.

## Algorithms

### Pocket (dc-cam::pocket)

Inputs: project, op. Output: cut blocks (then `sequence`).

1. **Resolve and nest** as Profile step 1–2: transformed closed profiles
   become `Contour`s; open paths warn `OpenPathSkipped`; `nest` gives
   regions (outer CCW, holes CW). Each region is one pocket; its holes are
   islands. A region nested inside an island is its own pocket (depth 2),
   cut as a separate region, ordered after the island by containment.
2. **Tool-centre region.** `r = diameter/2`, `inset = r + allowance_mm`.
   `tc = inset_region(region, inset)`: outer offset by −inset, islands by
   +inset, then boolean difference (i_overlay, non-zero), split into
   regions (an island can split a pocket into several). An empty result
   warns `PocketNoRoom` and the region is skipped. A miter join on the
   outer, round on islands (PureCutCNC's `jtMiter`/`jtRound` choice), so
   wall corners are reached as far as the tool allows and island corners
   are wrapped.
3. **Rest region** (`rest_of` set): let `R` be the earlier Pocket's tool
   radius plus its allowance. Its reachable area is
   `offset(inset_region(region, R), +R)` (what a disc of radius `R` swept
   along its tool-centre region covers). The rest region is
   `region − reachable`, grown by `+r` and intersected with `region`, so
   the small tool's wall band and corner wedges are cleared with full
   overlap into the already-cut area. If the rest region is empty, warn
   `RestOfClearsNothing` and emit nothing. The tool-centre region of step
   2 is then computed from the rest region. The reference operation's
   `allowance_mm` is included in `R` so a rough pocket with allowance 0.5
   leaves a 0.5 mm band the rest pass removes.
4. **Passes.** As Profile step 5 (`passes::depths`), with `step =
   op.step_down_mm.or(tool.step_down_mm)` and S1.
5. **Stepover** `s = diameter × step_over_fraction` (operation, else tool;
   validation guarantees (0, 1]).
6. **Offset strategy.** Build the ring tree once per tool-centre region:
   `children(node) = inset_region(node, s)`, recursively, until empty.
   Every node's outer and islands are rings. Emit blocks per pass,
   innermost first (PureCutCNC `inner-first`): for each root region, a
   depth-first post-order walk emits the leaf rings, then their parents,
   ending with the root's rings, which are the wall and island walls.
   With `wall_pass: first` the root's rings are emitted before the walk;
   with `none` the root's rings are omitted and the first child becomes
   the outermost ring (the pocket is undersize by `s`, as VCarve's "No
   Profile Pass" warns). Ring blocks of one pass share a `group` so
   `sequence` keeps the inner-first order. Each ring's `cleared` area is
   the ring's own region grown by `r`, so the link from a ring to the
   next one outward (at most `s` away, inside material the inner rings
   have already cleared plus the tool's own width) stays down under the
   `sequence` link rule; a link to a ring of another root region, or
   across an island, retracts. Each ring's start vertex is the vertex
   nearest the previous ring's end (`rotateContourToNearestEntry`).
7. **Raster strategy.** Rotate the tool-centre region by `−angle_deg`
   about the origin (so lines at `angle` become horizontal), scan
   horizontal lines from `min_y + s/2` to `max_y − s/2` in steps of `s`
   (PureCutCNC's scan; one line in a region narrower than `s`), intersect
   each with the region (even-odd over outer and islands; horizontal
   edges ignored, crossing rule `a.y ≤ y < b.y` or `b.y ≤ y < a.y`),
   rotate the segments back by `+angle_deg`. Lines alternate direction;
   each segment of a split line (around an island) is its own block.
   Links: a segment's block follows the previous one; `sequence` stays
   down when the straight link between them lies inside the tool-centre
   region (no island between) and is shorter than one diameter plus `s`,
   else retracts. The wall pass rings (root rings of step 6 with no
   children) are emitted last (or first) per pass. PureCutCNC cuts the
   boundary first on raster; VCarve's default is unstated; DreamCarve
   defaults to last (interim notes). **Y-up note:** PureCutCNC rotates by
   `−pocketAngle` in a Y-down frame, so its +45° raster is DreamCarve's
   −45° (135°); the golden `.camj` for `pocket-raster-45deg` uses
   `pocketAngle: -45`, and the case README says so.
8. **Direction.** The tool runs inside the pocket wall and outside each
   island. Spindle clockwise from above, Y-up: climb on the pocket wall
   traverses the wall ring **counter-clockwise** (VCarve: "Climb (CCW)"
   for pockets; phase 1's inside rule); climb on an island ring traverses
   it **clockwise** (an outside wall). Conventional is the reverse.
   Interior rings follow the wall's sense. Raster lines ignore direction.
   **Y-down note:** PureCutCNC's "conventional is a no-op on Clipper
   output" holds because its positive-area outers are machine-CW; in Y-up
   a positive-area (CCW) outer is climb, so DreamCarve reverses for
   conventional. This is a unit test, not a comment.
9. **Entry** (`entry` module, below) gives each block's descent; the
   block's `entry` point is where the descent ends, which for a ring is
   its start vertex and for a raster line its start. Helix and ramp are
   placed in the pass's tool-centre region (the material this pass
   removes).
10. **Corner relief** (`relief` module): after all passes, with
    `cleared = region inset by allowance` and `walls = tool-centre
    region's root rings`, collect corners and emit the notch blocks in
    their own group at the end, stepping down at the tool's step-down.
11. **Blocks → `sequence`** with containment pairs from `nest` depth.
12. **Footprint check** (S9): `footprint(toolpath, r)` must lie inside
    `region` (offset by `+EPS_MM`) and outside every island; else
    `CamError::OffMaterial`. Stock bounds as Profile step 8.

Coverage check (acceptance 19, 22): `union(footprint per pass) ⊇
inset_region(region, r)` within 0.05 mm, computed with i_overlay; the
difference's area must be below `0.05 × perimeter`. PureCutCNC's
`restrictFloorRoot` noise rule (`max(4 units, reach × 1e-3)`) becomes this
tolerance.

### Drill (dc-cam::drill)

1. Resolve targets. For each closed profile, **circle test**: let `c` be
   the centroid of the vertices; the profile is circular if every vertex
   and every arc's midpoint lies within `flatten_tolerance_mm` of the
   circle of radius `mean(|v − c|)` about `c`, and (for polylines with
   arcs) every arc's centre is within the tolerance of `c`. Non-circular
   or open profiles warn `DrillTargetNotCircle` and are skipped. Hole
   diameter `2 × mean radius` is kept for warnings: tool wider than the
   hole warns `DrillToolWiderThanHole`.
2. `top = thickness`, `bottom = thickness − depth` (S1 with allowance),
   `retract_z = min(safe_z, top + retract_mm)`, `peck = peck_depth_mm
   .or(tool.step_down_mm)` (validation guarantees one), `chip gap` from
   `retract`.
3. Order: nearest neighbour from the first circle in drawing order, ties
   by drawing order (D5).
4. Per hole, one block whose `descent` is empty and whose `cuts` are:
   - Simple: `Linear` down to `bottom` at plunge; (dwell) `Dwell(s)`
     pseudo-move; `Rapid` up to `retract_z`.
   - Peck, full retract: for `z_k = max(bottom, top − k·peck)`: `Linear`
     down to `z_k` at plunge, (dwell), `Rapid` up to `retract_z`, then
     `Rapid` down to `z_k + PECK_REENTRY_MM` (0.5 mm, a named constant:
     the tool re-enters the hole by rapid only through air it has
     already cut) and the next `Linear` continues from there, until
     `z_k == bottom`; final `Rapid` to `retract_z`. A controller's `G83`
     re-enters by its own documented gap; S11 compares with that gap as
     the tolerance (see "Canned cycles").
   - Chip break: as peck but each retract is `Rapid` up by `gap_mm` only
     and the next `Linear` resumes from there.
   The block's `entry` is the centre; `sequence` rapids to it at safe Z
   and descends to `retract_z` by rapid (above material), then the block's
   first cut starts at `retract_z`. Between holes the link is at safe Z.
5. `Move` gains `MoveKind::Dwell { seconds: f64 }` (no motion; emitted as
   `G4 P<seconds>` in seconds; the safety parser treats it as no motion).
6. `DrillHole` records per hole for the emitter, with `cycle` = `G81`
   (simple, no dwell), `G82` (simple + dwell), `G83` (peck full, no
   dwell), `G73` (chip break, no dwell), else `Expanded`.
7. S15: every `Rapid` that descends ends at or above the deepest Z this
   hole has been cut to plus `gap`, checked in `sequence`'s output test.

### Engrave (dc-cam::engrave)

1. Resolve targets: every profile, open or closed; no offset. A profile of
   fewer than two distinct points warns `EngravePathDegenerate`.
2. Passes as Profile step 5. `bottom` obeys S1 with the allowance.
3. Per path per pass one block: `cuts` are the path's moves at `z`; for a
   closed path the block ends at its start; for an open path, under
   `alternate_direction`, odd passes run the path reversed, the block's
   `entry` is the previous block's `exit`, and `descent_from_z_mm` is the
   previous pass Z, so `sequence` emits a straight plunge without a
   retract between them. Without alternation every pass starts at the
   path's first vertex (retract, rapid back, plunge).
4. No direction, no entry policy (plunge), no tabs. Containment pairs
   still apply to closed paths (an engraved outline inside another is
   cut first; harmless but deterministic).

### Entry (dc-cam::entry)

Ported from `entry.ts`. Inputs: the pass's clearance regions (tool-centre
regions where the tool may be at this Z), the target entry point, the
descent start Z, the pass Z, the tool diameter `D`, feed and plunge, the
cut direction and cut side.

- **Constants:** `DEFAULT_RAMP_ANGLE_DEG = 5`, `DEFAULT_HELIX_FRACTION =
  0.8`, `HELIX_SEGMENTS_PER_REV = 48`, `BOUNDARY_SAFETY = max(1e-4,
  0.1·D)`, `MAX_RAMP_RUN = 3·D`, `MAX_DESCENT_MOVES = 20_000`, 72 angle
  samples, 4 radius halvings, 12 endpoint candidates, 20 000 clearance
  cells.
- **Pitch** `π · d · tan(angle)` for path diameter `d`. **Feed scale**
  `min(1, plunge / (feed · sin(slope)))` on every descending move (S10).
- **Helix placement:** largest clearance circle (pole of inaccessibility,
  quadtree best-first, cached per region); start radius `min(D·f/2, D/2,
  circle_r − safety)`; try the tangent placement (72 angles; the helix
  touches the target point) then the reachable placement (quadtree search
  nearest the target, 12 endpoint tries over 24 angles); halve the radius
  up to 4 times. Success below the requested diameter warns
  `HelixDiameterReduced`. Descent: `ceil(depth/pitch · 48)` segments, Z
  linear, then one flat revolution, then a straight handoff to the target
  if the helix end is not on it. **Sense (Y-up):** tool inside a wall
  (pocket, inside profile, hole): climb → counter-clockwise, conventional
  → clockwise; tool outside a boss (outside profile): the reverse.
  **Y-down note:** PureCutCNC's `helixAngularDirection` returns the
  negation of these.
- **Pocket ramp placement:** 72 directions over a half circle through the
  target; longest chord inside the region, shrunk by `safety` at both
  ends; run at most `3·D` centred on the target and slid to stay inside;
  zigzag descent at `angle`, final partial run clamped; handoff to the
  target.
- **Profile ramp:** the same zigzag placement, in the Profile's waste
  domain (below), as PureCutCNC's edge routes do (`edge.ts`,
  `bandEntryPolicy` with `cutSide: 'internal'` for inside routes and
  `outsideEntryPolicy` with `'external'` for outside routes; the domain is
  the same one the lead uses). The ramp ends with a straight handoff to
  the block's entry point at pass Z. Slice 10: the run is anchored with
  one end on the entry point (the longest chord from it into the domain,
  shrunk by `safety` at the far end only, at most `3·D`) instead of
  centred on it, because only the kerf is cleared at the previous pass Z:
  each later pass starts its zigzag where the previous lap ended. A
  helix after the first pass rejoins the bore the previous pass's helix
  cut (target → helix end → helix start, flat), or descends from safe Z
  if that pass cut none. A contour whose start lies under a tab span
  plunges there (`EntryFallback`, `no_room`).
- **On line ramp:** an On line Profile removes only its own kerf, so the
  ramp runs along the cut path itself: from the start vertex, forward
  along the path for `run = min(3·D, L)` and back, descending at `angle`
  on each leg until pass Z is reached, the final partial leg clamped;
  arcs are split into chords within the flatten tolerance while Z
  changes. The pass then cuts the full path from the start vertex, so
  the ramped stretch is cut again at full depth (VCarve's Zig Zag ramp).
  On an open path shorter than one leg the ramp reverses at the path
  end. The feed scale of S10 applies to every leg.
- **Fallback chain:** helix → ramp → plunge, each step warning
  `EntryFallback` with `requested` and `used`. A plunge is always
  possible. `ramp` is the default for new projects (`init`, the chapter
  blocks), so a project whose shapes leave no room for a zigzag (a hole
  barely wider than the tool) will warn on every export until the user
  sets `plunge`; the warning says so.
- **Waste domain for Profile:** Inside → the hole's tool-centre region
  (outer offset by −r); Outside → the stock rectangle minus every target
  shape grown by `r`, minus every other region's tool-centre path grown
  by `r` (so neither ramp nor helix enters a neighbouring part or its
  kerf); On line → no domain: helix is a validation error, ramp runs
  along the path. Tabs are keep-outs in the domain at every Z below the
  tab top, as upstream (`tabKeepOutWithin`).

### Lead-in and lead-out (dc-cam::lead)

Ported from `xyLead.ts`, with the user's radius.

1. **Domain:** the waste side as above, intersected with the stock, with
   every other shape's tool-centre path grown by `r` subtracted. Points
   within `sqrt(2·radius·safety)` of the arrival use "inside domain";
   farther points must be `safety = 0.1·D` clear of every wall.
2. **Arrivals:** candidates are every edge of the tool-centre contour ×
   fractions `[0, 0.5, 0.25, 0.75]`; score = (corner turn within one `D`
   of the point, quantised to 5°; then distance from the previous cut's
   exit; then edge index; then fraction). Lowest wins: the seam avoids
   corners first, then travel.
3. **Lead-in:** for `radius` then `radius × [0.75, 0.5, 0.25]` and sweep
   `sweep_deg` then `[60°, 45°]` (only those below the user's sweep), for
   each arrival, for each free side (probe 0.05·D either side of the
   tangent): build the arc that arrives tangent at the point, sample it
   at chord budget `r·sin(step/2)/2`, require every sample in the domain
   and the arc's far end more than `0.5·radius` from the arrival. First
   fit wins. The contour is rotated to start at the arrival (a fraction
   inserts a vertex). Shrinking warns `LeadShrunk`; no fit warns
   `LeadDropped` and the block uses the plain start with `entry` at the
   vertex.
4. **Lead-out:** after the lap returns to the arrival, continue along the
   contour `overcut_mm` (0 → depart on the closing chord's tangent), then
   the mirror arc (same ladder); a failed exit warns `LeadDropped (reason:
   exit)` and the block ends on the contour.
5. The block's `descent` ends at the lead-in's staging point (the entry
   policy places the plunge/ramp/helix there); lead moves are `Linear`/
   `Arc` at the feed rate at `z_mm`, part of `cuts`. Tabs are applied to
   the contour portion only.

### Corner relief (dc-cam::relief)

Ported from `cornerRelief.ts`.

1. **Walls:** the tool-centre rings whose material side the notch enters;
   `cleared` loops: the region (with `clearedInside: true`) and islands
   (`false`) for a Pocket; the hole (true) for an inside Profile; the part
   outline (false) for an outside Profile.
2. **Corners:** canonicalise each loop so cleared material is on the
   left; at each vertex `turn = atan2(−cross(eA, eB), −dot(eA, eB))`;
   keep corners with `turn > RELIEF_MIN_TURN = 15°` (toward the cleared
   side). Both adjacent edges must be ≥ `2r`, else `ReliefCornerTooTight`.
   Descend point: where the bisector from the corner crosses the wall
   ring, at `t ∈ [r, r/sin(θ/2)]`; none → `ReliefCornerTooTight`.
3. **End point:** dogbone `C + bisector·r`; T-bone `C + u·r` along the
   longer adjacent edge (tie: the more X-parallel edge, then `+x`). Notch
   depth past the wall: `r(1 − sin(θ/2))` dogbone, `r` T-bone (tests).
4. **Pass:** after the main blocks, corners ordered nearest-neighbour from
   the last exit; for each corner whose descend point the main path has
   cut at the deepest level (else `ReliefCornerNotCut`), one block per
   level of `generateStepLevels(top, bottom, tool.step_down)`: plunge at
   the descend point, cut to the end, cut back. Blocks share one group;
   links between corners retract. A tool with no step-down skips the pass
   with `ReliefSkipped`.

### Sequence (dc-cam::sequence)

New, per D10 and research §5.5. Pure function of `SequenceInput`.

1. **Validate:** every block's `cuts` is at `z_mm` (tabs may rise above);
   each shape's passes are consecutive integers from 0.
2. **Units:** a *unit* is one shape's blocks at one pass (full depth
   first groups a shape's units into a *stack*; pass first groups all
   shapes' units of one pass into a *layer*). Blocks within a unit keep
   generator order (`group`).
3. **Order:**
   - `FullDepthFirst`: order shapes by a greedy walk: start from the
     first shape in drawing order; from the current shape's exit, pick the
     nearest *available* shape's entry (available = every shape it
     encloses, per `containment`, is finished; distance by
     `f64::total_cmp`; ties by drawing order). Emit all its passes top to
     bottom.
   - `PassFirst`: for pass k, walk the shapes as above but a shape is
     available at pass k only when every enclosed shape has finished pass
     k and, if this is the shape's freeing pass, has finished entirely.
     The walk for each pass starts from the previous pass's last exit.
   - Drill: one pass per hole; the two orders coincide.
4. **Links:** between consecutive blocks `a → b`:
   - same shape, next pass, `b.entry == a.exit` (within `EPS_MM`): no
     link; `b.descent` starts at `a.z_mm` (`descent_from_z_mm`), a
     straight plunge if empty;
   - same shape, next pass, different entry (lead-in moved the seam, or
     alternation): rapid up to safe Z, rapid to `b`'s descent start,
     descend;
   - `b.cleared` and `a.cleared` are both `Some`, the straight link lies
     inside `a.cleared ∪ b.cleared` at `b.z_mm == a.z_mm`, and its length
     ≤ `tool_diameter + stepover` (`SequenceInput` carries the threshold):
     stay down, `Linear` at feed (O3);
   - otherwise: rapid up to safe Z, rapid in XY, then `b.descent` from
     safe Z (or a plunge at the plunge rate).
   The first block is reached by a rapid to safe Z then XY; the last is
   followed by a rapid to safe Z.
5. **Output:** one `Toolpath`. Every `Rapid` is at safe Z except the Z
   rapids down to a drill's `retract_z` and the descents inside a block
   (which are `Linear` at feed/plunge, never `Rapid`).
6. **Invariants O1–O10** are the module's tests.

`generate_profile` moves onto `sequence` in the first phase 2 slice
(D10): its per-contour emission becomes blocks; the phase 1 goldens must
stay byte-identical for one-shape cases (O6) and set-identical per level
for the ring case.

### Operation-order warnings (dc-cam::order)

For operations `i < j`, both enabled: if `i` is a Profile whose final pass
reaches the stock bottom (depth ≥ thickness) and its side is Outside or
OnLine, every closed target of `j` whose transformed outline lies inside
one of `i`'s target outlines (point-in-polygon on the centroid plus no
crossing) yields `CutInsideFreedPart (op: j, earlier_op: i, instance)`.
An Inside Profile through the stock frees the slug of its hole, so a later
operation inside that hole warns too. Tool hint: the sequence of tools of
enabled operations in order; whenever a tool recurs after a different one,
`ToolChangeAvoidable (tool, ops)` once per tool.

### Arc fitting (dc-gcode::arcfit)

Ported from `arcFitting.ts`. Runs in `emit` after the toolpath is in
project space and before the machine transform (the transform is a
translation and signed axis map, never a mirror, so handedness survives;
the fitted centre is transformed like a point).

1. Runs: maximal sequences of consecutive `Linear` cutting moves at one Z
   with one feed (`Arc` moves break runs; they are already arcs). Fewer
   than 4 points → unchanged.
2. Greedy longest-first: for `end` from `min(n−1, i+127)` down to `i+3`,
   Kasa least-squares circle through points `i..=end`; project the centre
   onto the perpendicular bisector of the endpoints and set `r` from the
   start (endpoints exact); accept if every point is within `tolerance`
   of the circle, every angular step ≤ 90°, the largest step ≤ 4 × the
   median, the total sweep ≥ 0.5°, and the chord ≥ 0.15·r (or a full
   circle). Direction from the sign of the summed cross products
   (negative = clockwise = `G2`).
3. Split each fitted run into arcs of ≤ 90° with intermediate endpoints on
   the circle and the last endpoint exactly the source point; `I`,`J` per
   sub-arc from its own start.
4. After number formatting (machine coordinates, the post's decimals),
   check each arc as the controller will: with `ij`, snap `I`,`J` within
   ±2 quanta to minimise the radius disagreement; accept when
   `|r_end − r_start| ≤ 0.005 mm`, reject above 0.5 mm, else accept when ≤
   0.1 % of `r`; with `r` format, repair a chord/2 rounding shortfall of
   up to 2 quanta, else reject. A rejected sub-arc reverts its whole run to
   the original lines and warns `ArcFitFallback (op, runs)`. A full circle
   never emits as one arc (phase 1 `FullCircleArc`): a run that closes is
   split at 90° so no sub-arc's end equals its start.
5. Not fitted: rapids, plunges, dwells, descents (helix descents change
   Z; the flat bottom revolution of a helix *is* fitted, which is correct
   and matches a hand-written circle), drill moves, lead arcs (already
   `Arc`).

**S12 in the safety pass:** `parse_motion` returns each `G2`/`G3`; S6
accepts an arc where the toolpath has lines when the arc's start and end
are toolpath vertices, every toolpath vertex between them lies within
`tolerance + one unit in the last place` of the arc, the arc's sweep ≤
90°, and the arc's direction matches the sign of the vertices' turn.
`fitted_arcs_pass_s6` and `arc_off_the_vertices_fails_s6` are the tests.

### Canned cycles (dc-gcode::emit)

When the operation is a Drill, `use_canned_cycles` is true, the
definition's `canned_cycles` is `Some`, and every hole's `cycle` maps to a
command the definition has (`G81 → drill_command`, `G82 →
drill_with_dwell_command`, `G83 → peck_drill_command`, `G73 →
chip_break_drill_command`): rapid to the first hole's XY at safe Z (Z
first, then XY, as phase 1 rapids), write `retract_mode` (`G98`: return to
the initial Z, which is safe Z) once, then one line per hole: the command
word (modal), `X Y`, `Z` (bottom, machine coordinates), `R` (retract
plane), `Q` (peck for `G83`/`G73`), `P` (dwell seconds for `G82`), `F`
(plunge rate, once). Close with `cancel_command` (`G80`). Any hole whose
cycle is `Expanded`, or a missing command, makes the whole operation
expand (one warning `canned_cycle_expanded (op, reason)`), so a program
never mixes cycles and moves for one operation.

**Chip-break re-entry.** `G73` on LinuxCNC retracts by the controller's
`G73` setting, not by `Q`; Mach3 and UCCNC document `G73` as "retract by a
fixed amount". The expansion in the safety pass (S11) therefore uses
`chip_break.gap_mm` for the toolpath and the controller's documented
behaviour for the program; where the documentation gives no fixed value
(LinuxCNC's `G73` uses the `$` `[RS274NGC]` parameter), the definition
sets `chip_break_drill_command: null` and chip-break always expands. The
definitions README records this per controller.

**S11 expansion** (`safety.rs`): on `G81`–`G83`/`G73` lines, the parser
synthesises the motion the controller performs: rapid XY at the current
Z; rapid to `R`; `G81`: feed to `Z`, rapid to `R` (`G99`) or initial Z
(`G98`); `G82`: plus a dwell; `G83`: feed `Q` increments with a rapid to
`R` and back to the previous depth + the controller's re-entry gap
(documented per controller, written in the README) between; `G73`: feed
`Q` increments with a short rapid retract between. The expansion is then
compared with the Drill toolpath move for move (S6) with the re-entry gap
tolerance stated in the definition (`canned_cycles.reentry_gap_mm`, a new
definition field, `null` where the controller retracts fully). Only
controllers whose documentation states these semantics get canned-cycle
commands in their definition.

### Post-processor definitions (dc-gcode/definitions)

Four new files, started from PureCutCNC's and then checked line by line.
The README gets one section per controller with the document and section
each line was verified against. Checks required before merge (the slice's
tests assert the resulting bytes):

| Controller | Source to cite | Lines to verify |
| --- | --- | --- |
| LinuxCNC | LinuxCNC G-code reference (`gcode.html`), "G-code Quick Reference", `G81`–`G83`, `G73`, `G98`/`G99`, `M2`/`M30`, `M6`, `G43`, `G53`, `G4 P` seconds, comments `( )` and `;`, `%` | header `G90 G17 G40 G49 G80 G94`; `G21`/`G20`; `G54`; `M3 S`; `G4 P` (seconds); tool change `M5`, `T{n} M6` (no `G43 H`: tool length offsets need a tool table DreamCarve does not write; the user sets Z zero per tool as on grblHAL); footer `M5`, `M2`; `.ngc` |
| Mach3 | Mach3 Mill manual (ArtSoft, "Mach3Mill 1.84"), G and M code chapters | `%`, `O0001`, line numbers `N10` step 10; `G90 G94 G17 G40 G49 G80`; `G4 P` **milliseconds** (Mach3 documents `P` in ms unless configured otherwise; the definition's dwell command is `G4 P{milliseconds}` with a `dwell_units` field `ms`); `M6 T{n}` then `M0`? (Mach3 `M6` runs the tool-change macro; `pause_after_change` false); `M30`; `.tap` |
| UCCNC | UCCNC Users Manual (CNCdrive), G-code and M-code lists | as Mach3 where the manual agrees; `G4 P` seconds per the UCCNC manual (verify; differs from Mach3); `.txt` |
| generic | RS274/NGC (NIST, Kramer, Proctor, Messina 2000) | `( )` comments, no line numbers, `G90 G17 G94`, `G21`/`G20`, `G54`, `M3 S`, `G4 P` seconds, no tool change `M6` (tool change block `M5`, `M0` with message, so `"m6"` mode is refused), `M30`, trailing zeros off, `.nc` |

`SpindleDwell` gains `units: Seconds | Milliseconds` so the Mach3 dwell is
written right; phase 1 definitions set `seconds` in the migration (format
change inside the embedded post-processor; covered by v6 → v7). `G4`
dwell for Drill uses the same field. Tool-change height, `G53` parking and
`G43` stay out (#73).

### DXF import (dc-import::dxf)

Parsing by the `dxf` crate (MIT). `import_dxf(bytes, options) ->
Result<(Vec<FeatureDefinition>, Vec<ImportWarning>), ImportError>`,
`options = { units: Option<Units>, join_tolerance_mm: f64, layers:
Option<Vec<String>> }`.

1. **Units:** `$INSUNITS` 1 → inch, 2 → feet, 3 → miles, 4 → mm, 5 → cm,
   6 → m (scale to mm); anything else or absent → `ImportError::UnitsUnknown`
   unless `options.units` is given. `init` maps that error to "pass
   `--units mm|inch`".
2. **Entities (model space only; `67 == 1` and paper-space layouts
   skipped):** `LINE` → open 2-vertex polyline; `CIRCLE` → closed
   polyline of two bulge-1 vertices at `(cx ± r, cy)`; `ARC` → open
   polyline, one vertex pair with bulge `tan(sweep/4)` (DXF arcs are CCW
   from start to end angle; sweep normalised to (0, 2π]); `LWPOLYLINE`
   and `POLYLINE`/`VERTEX` → vertices with bulges (bulge on the vertex it
   starts from; flag 1 = closed; mesh flags 16/64 skipped with a warning);
   `SPLINE` → rational B-spline evaluated by de Boor into a `kurbo`
   `BezPath` (degree, knots, weights; fit points as fallback) then
   `flatten` at the project tolerance; `ELLIPSE` → `kurbo::Ellipse` arc
   flattened; `INSERT` → block entities under `T(insert) · R(rotation) ·
   S(xscale, yscale) · T(−base)`, recursively (depth ≤ 12, else a
   warning), arcs under a non-uniform scale flattened (as SVG), under a
   mirror their bulges negated; `$MODEL_SPACE`, `$PAPER_SPACE`, names
   starting `_`, and layers `ASHADE`/`HATCH` skipped silently as upstream.
   Everything else counts in `DxfEntitiesSkipped { counts: BTreeMap<String,
   usize> }`.
3. **No Y flip.** Coordinates are already Y-up; `y' = y × scale`.
4. **Layers:** the entity's layer, inheriting the INSERT's for layer `0`
   inside a block; `options.layers` filters by name (layer `0` shapes
   match `"0"`).
5. **Stitch:** closed shapes pass through; open polylines on the same
   layer are joined end to end when an endpoint pair is within
   `join_tolerance_mm` (nearest first, four end combinations, the joined
   piece translated by the gap), repeated to a fixed point; a chain whose
   ends meet within the tolerance is snapped closed. Identical shapes on
   one layer (same vertices within 1e-6, either direction) are deduped.
6. **Definitions:** one per shape, named `<layer>` (numbered when a layer
   has several), one instance each with identity transform, as SVG.
   Coordinate limit (`MAX_COORDINATE_MM`) as SVG.
7. `init` accepts `.dxf` by extension; `--units`, `--join-tolerance`,
   `--layer` (repeatable) are DXF-only flags.

### Fusion import (dc-library::fusion)

Autodesk publishes the tool library format: `cam.autodesk.com/tools/tools/
Tooling Schema/JSON Schema.zip` holds `FusionToolLibrarySchemas.html` (the
JSON schema, rendered), `Tool Library Schema Information.docx` (geometry
parameters per tool type with a JSON snippet each), `sample library.json`
(version 36, every tool family, 34 tools) and `Sample Metric Tools- With
Presets.json` (version 14, 235 milling and hole-making tools with
presets). The `.tools` file is that JSON zipped (`ReadMeFirst.txt`). The
archive carries no license statement, so its files are reference only
and are not committed (maintainer decision, 2026-10-20). The fixtures
under `tests/fixtures/fusion/` are hand-written in the schema's structure:
`sample.tools` (zipped) and `sample.json` with one tool of each supported
type, one of each skipped family, a tool with two presets, a drill preset
without `v_f`, an inch tool, and a chamfer mill with `TA` 45. The parser accepts unknown keys everywhere
(the schema itself allows additional properties on most objects and new
versions add fields) and requires only what it maps.

From the schema:

- Root: `{ "version": integer ≥ 11, "data": [ … ] }`. Each item is a
  Tool (Turning, Milling, Cutting, Depositing, Probe, Hole Making) or an
  Adaptive Item (holder). `data` missing → `LibraryError::NotAFusionLibrary`.
- `type` is an enum per family. Milling: `"flat end mill"`, `"ball end
  mill"`, `"bull nose end mill"`, `"chamfer mill"`, `"radius mill"`,
  `"tapered mill"`, `"dovetail mill"`, `"face mill"`, `"slot mill"`,
  `"lollipop mill"`, `"form mill"`, `"thread mill"`, `"circle segment
  barrel|lens|oval|taper"`. Hole making: `"drill"`, `"spot drill"`,
  `"counter sink"`, `"counter bore"`, `"reamer"`, `"tap left hand"`,
  `"tap right hand"`. Turning `"turning *"`, `"probe"`, `"holder"`.
- `unit`: `"millimeters"` | `"inches"` ("Tool unit").
- `geometry`: `DC` (cutting diameter, number > 0), `NOF` (flute count,
  ≥ 0), `TA` (taper angle, "included angle between generatrixes in the
  axial plane section"), `SIG` (point angle of hole-making tools), `RE`
  (corner radius), `LCF` (flute length), `tip-diameter`, `tip-length`,
  `SFDM`, `OAL`, `LB`.
- `start-values.presets[]`: `name` (required), `guid`, `n` (spindle
  speed), `v_f` (cutting feedrate), `v_f_plunge` (plunge feedrate),
  `v_f_ramp`, `v_f_leadIn`, `v_f_leadOut`, `v_f_retract`, `f_z`, `v_c`,
  `stepdown` and `stepover` (lengths in `unit`), `use-stepdown` and
  `use-stepover` ("whether the selected preset should override an
  operation's stepdown/stepover"; `stepover` is required when
  `use-stepover` is true), `tool-coolant`, `material`.
- `post-process`: `number`, `length-offset`, `diameter-offset`, `turret`,
  `manual-tool-change`, `live`, `break-control`, `comment`.
- `description`, `vendor`, `product-id`, `product-link`, `guid`, `BMC`
  (body material code), `GRADE`, `holder` (ignored).

Mapping:

1. **Container:** bytes starting `PK` → zip (`zip` crate, MIT); the entry
   named `tools.json` (or the only `.json`) is the document. Otherwise the
   bytes are the document.
2. **Per tool, by `type`:** `"flat end mill"`, `"bull nose end mill"`
   (corner radius `RE` ignored; the tool name gets ` (bull nose r<RE>)`),
   `"slot mill"` → `EndMill`; `"ball end mill"` → `BallNose`; `"chamfer
   mill"` → `VBit { included_angle_deg }`; `"drill"`, `"spot drill"`,
   `"center drill"`, `"counter sink"`, `"counter bore"`, `"reamer"` →
   `Drill`; every other milling, hole-making, turning, probe and holder
   type → `Skipped { UnsupportedType(type) }`; a string outside the
   schema's enums → `Skipped { UnknownType(type) }`.
   - **Chamfer angle.** `TA` is the half angle: `included_angle_deg =
     2·TA`, so a 90° V-bit carries `TA` 45. Settled by the maintainer
     against IDC Woodcraft's published Fusion library (the schema's
     wording, "included angle between generatrixes", reads otherwise, and
     is noted in the module doc so nobody re-derives it).
3. **Units:** every length (`DC`, `stepdown`, `stepover`) and feed
   (`v_f`, `v_f_plunge`) is in `unit` and converted to mm or mm/min; `n`
   is RPM; `NOF` → flutes (0 or absent → 1, noted in the report).
   `stepover` becomes `stepover / DC` (a fraction, clamped to (0, 1] with
   a report note when above 1); `stepdown` → `step_down_mm`. Both only
   when their `use-*` flag is true or absent.
4. **Presets:** one `Tool` per preset: name `description` for one preset,
   `"<description> (<preset name>)"` for several; `source =
   "fusion:<tool guid>:<preset guid or index>"` so a re-import recognises
   it. No presets → one tool with feeds unset. Missing `n`, `v_f` or
   `v_f_plunge` leave the field `None` (never defaulted; export refuses
   until set), and the report says so. A drill preset usually has
   `v_f_plunge` and no `v_f` (Autodesk's sample): the tool imports with
   `feed_mm_min` unset, which Drill never needs but validation reports
   for a tool a Profile uses.
5. `DC ≤ 0` or absent → `Skipped { NoDiameter }`.
5a. **Tool number:** `post-process.number` > 0 → `Tool::number`; 0 or
   absent → `None`. Every preset of one Fusion tool shares the number,
   so a library with several presets of one tool holds several tools
   with the same number; `tools add` copies the number, and validation
   reports the clash only when two such tools are used in one project.
6. **Report:** `tools import` prints one line per tool: `imported`,
   `already present`, or `skipped: <reason>`, then the counts.

**Tool numbers** (functional open question 7, decided 2026-10-20): the
optional field is added in v7 (see "Data model", `Tool::number`). Why:
Fusion's `post-process.number` and VCarve's Tool Number are explicit and
stable per tool, which a tool table on LinuxCNC or Mach3 needs (`T5 M6`
must match the machine's slot 5), and adding the field later would cost a
second migration.

### Tool library CLI

- `tools list [--json]`: id, name, cutter, diameter, feeds, source.
- `tools add <project.dcv> <id-or-name>`: copies the tool into
  `project.tools` with a fresh `ToolId` (`t<n>`), saving the project
  through `dc-project::save`; ambiguous names list the matches.
- `tools import <file> [--replace]`: Fusion or DreamCarve JSON (by
  content, not extension); `--replace` overwrites a tool with the same
  `source`, otherwise it is `already present` when equal and `imported as
  <name> (2)` when different.
- `tools export <file>`: `dc-library::save`.
- `tools remove <id>`.
- `config path`: prints the config directory, the config file and the
  library file, and whether each exists.

All file I/O in the CLI; `dc-library` takes and returns bytes.

### CLI additions

```
dreamcarve init <svg|dxf> [-o] [--post <id>] [--units mm|inch] [--join-tolerance <mm>] [--layer <name>]...
dreamcarve export <dcv> [-o] [--post <id>] [--one-program | --programs-per-tool]
dreamcarve check <dcv>
dreamcarve posts
dreamcarve tools {list|add|import|export|remove} ...
dreamcarve config path
```

`init` writes `arc_fit_tolerance_mm: 0.01`, the user config's
`tool_change_mode` into the post-processor, and one Profile as in phase 1.
It does not write Pocket, Drill or Engrave operations: the user adds them,
following the chapters, which each carry a complete JSON block to paste.
`export` and `check` run `order::check` after generation and print its
warnings with the others. `export` prints, per program, the operations in
it and the warnings per operation, each as `<code>: <message>`.

## Module layout

```
crates/dc-geometry/src/{..., boolean.rs (i_overlay union/difference/intersection on Regions), circle.rs (circle test, Kasa fit)}
crates/dc-project/src/{model.rs (+ PocketOp, DrillOp, EngraveOp, Entry, Lead, CornerRelief, MachiningOrder), migrate.rs (v6_to_v7), validate.rs (+ rules above), post_processor.rs (+ use_canned_cycles, SpindleDwell::units, CannedCycles::reentry_gap_mm)}
crates/dc-cam/src/{lib.rs, toolpath.rs (+ Dwell), block.rs (CutBlock), sequence.rs, order.rs, profile.rs (blocks; entry, lead, relief hooks), pocket.rs, pocket/{rings.rs, raster.rs, rest.rs}, drill.rs, engrave.rs, entry.rs, lead.rs, relief.rs, footprint.rs, passes.rs, tabs.rs, overlap.rs, stock.rs, warning.rs}
crates/dc-gcode/src/{emit.rs (+ canned cycles, split modes), arcfit.rs, safety.rs (+ S9–S15), parse.rs (+ G2/G3 fitted, G8x expansion, G4), definitions/{README.md, grblhal.json, grbl.json, linuxcnc.json, mach3.json, uccnc.json, generic.json}}
crates/dc-import/src/{svg.rs, dxf.rs, dxf/{entities.rs, spline.rs, stitch.rs}}
crates/dc-library/src/{lib.rs, library.rs, fusion.rs, error.rs}
crates/dreamcarve/src/{main.rs, cli.rs, init.rs, export.rs, check.rs, tools.rs, config.rs}
tests/golden/{pocket-offset-rect, pocket-offset-island, pocket-raster-0deg, pocket-raster-45deg, pocket-rough-then-rest, drill-simple, drill-peck, engrave-open-closed, profile-ramp, profile-helix-inside, profile-lead, profile-dogbone, order-full-depth-first, order-pass-first, post-linuxcnc-coaster, post-mach3-coaster, post-uccnc-coaster, post-generic-coaster}/
tests/fixtures/{plaque.svg, plaque.dxf, dxf/*.dxf, fusion/{sample.tools, sample.json}, dcv/v6/*.dcv}
```

New dependencies: `i_overlay` (MIT, booleans; already a settled decision),
`dxf` (MIT), `zip` (MIT, `default-features = false`, deflate only),
`directories` (MIT/Apache-2.0), `toml` (MIT/Apache-2.0). `NOTICE` entries
as their licenses require.

## Test plan

Names are `crate::module::test`. "(port)" marks a PureCutCNC test to port;
"(golden)" a corpus comparison. Every emitted-motion test runs
`assert_safe`.

**dc-geometry**
- `boolean::difference_of_rect_and_circle_has_one_hole`, `boolean::union_of_touching_rects_is_one_region`, `boolean::intersection_of_disjoint_is_empty`.
- `circle::sixteen_facets_are_a_circle_within_tolerance`, `circle::two_bulge_arcs_are_a_circle`, `circle::ellipse_is_not_a_circle`, `circle::kasa_fit_recovers_centre_and_radius`.

**dc-project**
- `migrate::v6_profile_gains_phase_2_fields_with_defaults` — fixture loads with `machining_order: full_depth_first`, `entry: plunge`, `lead: null`, `corner_relief: none`, `arc_fit_tolerance_mm: 0.01`, `use_canned_cycles: false`.
- `migrate::v6_fixture_exports_phase_1_bytes_with_fitting_off`.
- `serialize::tool_number_round_trips`, `validate::duplicate_tool_number_is_error`, `validate::set_number_clashing_with_positional_is_error`, `validate::tool_number_zero_is_error`, `emit::set_tool_number_is_written_as_t_word_and_file_name`, `emit::unset_tool_number_falls_back_to_position`.
- `serialize::pocket_drill_engrave_round_trip`, `serialize::entry_variants_serialize_as_string_or_object`, `serialize::clearance_strategy_raster_carries_angle`.
- `validate::rest_of_must_be_an_earlier_enabled_pocket_with_the_same_targets`, `validate::step_over_fraction_outside_unit_interval_is_error`, `validate::pocket_without_step_down_is_error`, `validate::peck_without_depth_or_tool_step_down_is_error`, `validate::helix_on_on_line_profile_is_error`, `validate::lead_radius_must_be_positive`, `validate::ramp_angle_range`, `validate::canned_cycles_on_definition_without_them_is_error`, `validate::drill_with_v_bit_warns`, `validate::relief_without_step_down_warns`.

**dc-cam — sequence (O1–O10)**
- `sequence::hole_is_cut_before_its_outline`, `sequence::island_is_cut_before_the_hole_around_it`, `sequence::four_levels_cut_innermost_first` (O1).
- `sequence::orders_cut_the_same_moves` (O2).
- `sequence::links_between_shapes_retract_to_safe_z`, `sequence::stay_down_link_only_inside_cleared_area` (O3).
- `sequence::full_depth_first_z_transitions` (port: `testPocketFeatureFirstOrder`, `[-2,-4,-2,-4]`), `sequence::pass_first_z_transitions` (port: `testPocketLevelFirstOrder`, `[-2,-4]`) (O4).
- `sequence::equidistant_shapes_keep_drawing_order`, `sequence::nearest_neighbour_orders_a_c_b_as_a_b_c` (port: `testPocketFeatureFirstNearestBlockOrder`), `sequence::same_input_twice_is_identical` (O5).
- `sequence::single_shape_is_identical_in_both_orders` (O6).
- `sequence::pass_first_waits_for_enclosed_shapes_freeing_pass`.
- `profile::one_shape_golden_is_byte_identical_after_sequence` (phase 1 goldens unchanged).
- `tabs::final_pass_has_exactly_four_raised_segments_at_tab_z` extended to both orders (O9).

**dc-cam — pocket**
- `pocket::offset_rings_are_concentric_and_spaced_by_stepover` — 100×50, 6.35 mm, 40 %: ring gaps 2.54 ± 0.01.
- `pocket::offset_cuts_inner_rings_before_the_wall` (port: "offset cuts inner loops before wall-adjacent loops").
- `pocket::wall_pass_first_last_none` — ring order and the undersize with `none`.
- `pocket::footprint_stays_inside_region` (S9), `pocket::footprint_stays_outside_island` (S9).
- `pocket::offset_coverage_reaches_inset_region` — union of footprints ⊇ inset by r within 0.05 (port: `pocketClearance.test.ts` "no stock left" spirit).
- `pocket::island_wall_runs_opposite_to_pocket_wall` (port: `pocketFinishIslandDirection.test.ts`).
- `pocket::climb_on_pocket_wall_is_counter_clockwise`, `pocket::climb_on_island_is_clockwise`, `pocket::conventional_reverses_both` (Y-up re-derivation).
- `pocket::tool_wider_than_region_warns_and_skips`.
- `pocket::island_splits_region_into_two_tool_centre_regions`.
- `pocket::pocket_inside_island_is_cut_after_island`.
- `pocket::raster_lines_at_zero_degrees_are_horizontal_and_spaced`, `pocket::raster_lines_at_45_degrees`, `pocket::raster_lines_alternate_direction`, `pocket::raster_link_stays_down_inside_region`, `pocket::raster_retracts_across_island`, `pocket::raster_segments_split_around_island`, `pocket::raster_ignores_direction_wall_pass_honours_it`.
- `pocket::allowance_leaves_material_on_walls` — footprint stays `allowance` from the wall.
- `pocket::rest_region_is_corners_and_wall_band`, `pocket::rest_coverage_with_rough_reaches_small_tool_inset` (acceptance 22), `pocket::rest_of_that_clears_nothing_warns`, `pocket::rest_footprint_stays_inside_region` (S14).
- `pocket::step_down_from_tool_when_operation_unset`, `pocket::depth_below_bottom_is_error`.
- `pocket::levels_rapid_at_safe_z_before_plunging` (port: "level transitions rapid at safe Z").

**dc-cam — drill**
- `drill::simple_is_one_plunge_and_retract`, `drill::dwell_adds_dwell_move_at_bottom`, `drill::peck_full_retracts_above_hole_top`, `drill::chip_break_retracts_by_gap`, `drill::peck_depth_from_tool_step_down`, `drill::last_peck_is_shallower_never_deeper`.
- `drill::orders_by_nearest_neighbour` (port: `testDrillingOrdersByNearestNeighbor`), `drill::tie_breaks_by_drawing_order` (port), `drill::starts_from_first_in_drawing_order` (D5; differs from upstream's origin start, documented).
- `drill::non_circle_target_warns_and_is_skipped`, `drill::four_arc_circle_and_facet_circle_both_detected`, `drill::slot_is_not_a_circle`.
- `drill::tool_wider_than_hole_warns`.
- `drill::retract_above_safe_z_is_clamped` (port: `drillingRetract.test.ts` "capped at safe Z"), `drill::retract_below_surface_is_raised_and_warned` (port).
- `drill::rapids_descend_only_into_cut_depth` (S15; port: `assertRetractSafe`).
- `drill::holes_record_canned_kind` — simple → G81, simple+dwell → G82, peck full → G83, chip break → G73, peck+dwell → Expanded.
- `drill::depth_beyond_bottom_needs_allowance` (S1).

**dc-cam — engrave**
- `engrave::open_path_is_traced_on_the_line`, `engrave::closed_path_returns_to_start`, `engrave::passes_step_down_to_depth`, `engrave::alternate_direction_reverses_odd_passes_without_retract`, `engrave::no_alternation_retracts_between_passes`, `engrave::any_cutter_is_allowed`, `engrave::degenerate_path_warns`, `engrave::depth_obeys_material_bottom` (S1).
- `engrave::direct_mode_regression` (port: `testDirectModeRegression`), `engrave::region_clips_open_path` is **not** ported (no region masks).

**dc-cam — entry**
- `entry::pitch_from_ramp_angle` (port), `entry::feed_scale_keeps_vertical_rate_under_plunge` (port; S10), `entry::largest_clearance_circle_avoids_island` (port).
- `entry::helix_diameter_clamped_to_tool_and_warns` (port), `entry::helix_bounded_by_clearance_and_warns` (port), `entry::helix_stays_outside_islands` (port), `entry::helix_descent_revolutions_plus_flat_bottom` (port).
- `entry::helix_sense_climb_inside_is_ccw`, `entry::helix_sense_climb_outside_is_cw` (re-derived; port of "maps climb/conventional direction" with the sign flipped and a comment saying why).
- `entry::narrow_slot_falls_back_to_ramp` (port), `entry::no_room_falls_back_to_plunge` (port), `entry::shallow_angle_move_count_is_bounded` (port), `entry::ramp_run_is_capped_at_three_diameters` (port), `entry::ramp_stays_clear_of_boundary` (port), `entry::helix_placed_near_target_not_roomiest_spot` (port, #581).
- `entry::inside_profile_ramp_zigzags_in_the_hole` (port: `testTheInsideEntryStaysInsideTheOpening`), `entry::outside_profile_ramp_stays_off_part_and_obstacles` (port: `testTheOutsideEntryStaysOffThePartAndItsObstacles`), `entry::every_level_is_entered_through_the_policy` (port), `entry::no_descent_reaches_depth_in_one_plunge` (port), `entry::tabs_are_kept_out_of_the_entry` (port), `entry::unplaceable_entry_falls_back_out_loud` (port), `entry::on_line_ramp_runs_along_the_path_and_back`, `entry::on_line_lap_recuts_ramped_stretch_at_full_depth`, `entry::ramp_default_angle_is_15_degrees`.
- `entry::outside_profile_helix_avoids_neighbouring_part`.
- `entry::plunge_is_default_and_unchanged_from_phase_1`.

**dc-cam — lead**
- `lead::entry_arrives_tangent_to_the_ring` (port), `lead::exit_departs_tangent` (port), `lead::join_avoids_corners_then_takes_nearest` (port), `lead::filleted_corner_ranks_like_mitred` (port), `lead::circle_falls_through_to_travel` (port), `lead::radius_ladder_takes_widest_that_fits` (port, from the user's radius), `lead::domain_rejection_leaves_no_lead_and_warns` (port + `LeadDropped`), `lead::swept_sampling_sees_between_vertices` (port), `lead::rotate_ring_for_lead_preserves_perimeter` (port), `lead::determinism` (port).
- `lead::overcut_continues_past_start_before_exit`, `lead::lead_out_false_ends_on_contour`, `lead::lead_runs_at_feed_rate_at_pass_z`, `lead::lead_shrunk_warns_with_both_radii`, `lead::lead_stays_off_neighbouring_shapes_and_stock_edge`, `lead::tabs_apply_to_contour_not_lead`.

**dc-cam — relief**
- `relief::dogbone_ends_on_bisector_at_radius` (port), `relief::t_bone_ends_on_longer_edge_at_radius` (port, placement rule changed), `relief::notch_depth_past_wall` (port), `relief::right_angle_t_bone_leaves_perpendicular_wall` (port), `relief::clears_whole_wedge_at_every_angle` (port, both styles), `relief::stopping_short_leaves_corner` (port).
- `relief::square_pocket_yields_four_corners_and_descend_points` (port), `relief::cleared_outside_loop_relieves_concave_only` (port), `relief::too_tight_corner_rejected_with_warning` (port), `relief::none_collects_nothing` (port), `relief::uncut_corner_emits_no_motion_and_warns` (port).
- `relief::pocket_square_clears_all_four_corners_eight_cuts` (port), `relief::island_reflex_corners_relieved_convex_not` (port), `relief::arcs_and_fillets_produce_no_relief` (port), `relief::steps_down_at_tool_step_down` (port), `relief::no_step_down_skips_with_warning` (port), `relief::allowance_sizes_relief_to_the_wall_left` (port), `relief::rough_with_allowance_then_rest_with_relief_clears_corner` (port).
- `relief::inside_profile_relieves_hole_corners` (port), `relief::outside_profile_relieves_concave_part_corners` (port), `relief::corner_under_tab_is_skipped` (port).
- `relief::footprint_of_notch_stays_in_removed_material` (S9).

**dc-cam — order**
- `order::cut_inside_freed_part_warns` (O8), `order::inside_before_outside_is_quiet` (O8), `order::inside_profile_through_frees_its_slug`, `order::operations_keep_project_order`, `order::non_adjacent_same_tool_hints`, `order::adjacent_same_tool_is_quiet`.

**dc-gcode — arcfit**
- Ports from `arcFitting.test.ts`: `accepted_circular_run_gives_four_arcs`, `clockwise_is_g2`, `rejects_straight_run`, `rejects_z_changing_run`, `rejects_mixed_feed_run`, `rejects_discontinuous_run`, `splits_around_rapids`, `short_run_stays_linear`, `rejects_ambiguous_direction`, `rejects_high_residual_ellipse`, `splits_large_arc_at_90_degrees`, `split_arc_offsets_relative`, `rejects_near_collinear_run`, `rejects_long_straight_with_corner_fragment` (inch tolerance), `accepts_ordinary_arc_after_collinear_gate`, `full_circle_loop_is_four_arcs`, `partial_arc_with_straight_leads`, `adjacent_runs_share_endpoints_exactly`, `fitted_arcs_pass_through_source_endpoints`, `ij_derived_from_previous_emitted_point` (#447), `invalid_run_falls_back_to_original_moves`, `fallback_leaves_valid_runs_alone`, `r_format_rejects_unrepresentable_chord`, `inch_output_uses_millimetre_budget`, `snapping_never_worsens_agreement`, `r_format_repairs_rounding_shortfall_only`.
- `arcfit::coaster_circle_exports_as_at_most_eight_arcs_per_lap`, `arcfit::null_tolerance_reproduces_phase_1_program`, `arcfit::helix_descent_is_not_fitted_flat_revolution_is`.
- `safety::fitted_arcs_pass_s6`, `safety::arc_off_the_vertices_fails_s6`, `safety::arc_over_90_degrees_fails_s12` (S12).

**dc-gcode — emit, canned cycles, posts**
- `emit::drill_simple_writes_g81_per_hole_after_g98_and_ends_g80` (port: `testCannedSimpleG81`), `emit::dwell_writes_g82_with_p` (port), `emit::peck_writes_g83_with_q` (port), `emit::chip_break_writes_g73_with_q` (port), `emit::grbl_never_writes_canned_cycles` (port: `testRegressionGrblNoCannedCycles`), `emit::canned_cycles_off_by_default`, `emit::mixed_cycles_expand_whole_operation_with_warning`, `emit::expanded_dwell_writes_g4`, `emit::mach3_dwell_is_milliseconds`.
- `safety::canned_cycle_expansion_matches_drill_toolpath` (S11), `safety::canned_cycle_below_floor_fails_s1`, `safety::canned_cycle_rapid_into_uncut_fails`.
- `safety::ramp_steeper_than_plunge_fails_s10`, `safety::helix_feed_scaled_passes_s10`.
- `safety::pocket_cut_outside_region_fails_s9` (on the toolpath via `footprint`).
- `post::six_definitions_validate`, `post::linuxcnc_mach3_uccnc_generic_differ_only_in_dialect` (acceptance 36), `post::mach3_and_uccnc_number_every_line`, `post::generic_strips_trailing_zeros`, `post::generic_refuses_m6_mode`, `post::no_definition_has_g43`, `post::every_definition_line_is_cited_in_readme` (a test that scans the README for each non-placeholder header/footer/tool-change line).
- `emit::one_program_flag_writes_m6_on_grblhal`, `emit::one_program_flag_on_grbl_is_error`, `emit::programs_per_tool_flag_overrides_m6_mode_without_saving`.

**dc-import — dxf**
- `dxf::insunits_mm_inch_cm_m_scale_correctly`, `dxf::missing_insunits_is_error_without_override`, `dxf::units_override_wins`.
- `dxf::line_arc_circle_lwpolyline_import`, `dxf::arc_keeps_centre_and_radius_as_bulge`, `dxf::circle_is_two_bulge_arcs`, `dxf::bulge_sign_is_not_flipped` (no Y flip), `dxf::polyline_with_vertex_entities`, `dxf::closed_flag_closes_polyline`, `dxf::spline_flattens_within_tolerance`, `dxf::ellipse_flattens_within_tolerance`.
- `dxf::insert_applies_translation_rotation_scale`, `dxf::nested_insert_accumulates`, `dxf::mirrored_insert_negates_bulges`, `dxf::non_uniform_scaled_arc_is_flattened`, `dxf::insert_depth_limit_warns`, `dxf::system_blocks_and_layers_skipped`.
- `dxf::paper_space_entities_skipped`, `dxf::unsupported_entities_counted_in_one_warning`.
- `dxf::layer_names_become_shape_names`, `dxf::layer_filter_keeps_only_named_layers`, `dxf::block_entities_on_layer_0_inherit_insert_layer`.
- `dxf::four_lines_stitch_into_closed_shape`, `dxf::gap_beyond_tolerance_stays_open`, `dxf::stitching_does_not_cross_layers`, `dxf::identical_shapes_are_deduped`.
- `dxf::plaque_dxf_matches_plaque_svg_within_0_01` (acceptance 34), `dxf::coordinate_limit_applies`.

**dc-library**
- `library::round_trip_is_byte_identical`, `library::add_detects_already_present`, `library::find_by_id_then_unique_name`, `library::ambiguous_name_is_error`, `library::validation_rules_from_upstream` (port of `parseToolLibraryFile` rules: positive diameter, flutes ≥ 1, stepover in (0, 1], V-bit angle in (0, 180)).
- `fusion::zip_and_plain_json_both_read`, `fusion::flat_end_mill_maps_with_geometry_and_preset`, `fusion::inch_tool_converts_to_mm`, `fusion::chamfer_mill_is_v_bit_with_double_taper`, `fusion::drill_types_map_to_drill`, `fusion::two_presets_become_two_tools`, `fusion::no_presets_imports_with_feeds_unset`, `fusion::missing_feed_stays_none`, `fusion::stepover_length_becomes_fraction`, `fusion::use_stepdown_false_leaves_step_down_unset`, `fusion::unsupported_types_skipped_by_name`, `fusion::unknown_keys_are_ignored`, `fusion::not_a_library_is_error`, `fusion::sample_library_imports_every_supported_type_and_skips_the_rest` (the hand-written fixture: flat, ball, bull nose and slot mills, chamfer mill, drill, spot drill, counter sink, counter bore, reamer import; thread mill, tap, face, form, lollipop, dovetail and tapered mills, a turning tool, a probe and a holder skipped by name), `fusion::chamfer_mill_ta_45_is_a_90_degree_v_bit`, `fusion::drill_preset_without_v_f_imports_with_feed_unset`, `fusion::tool_number_is_read_when_non_zero`, `fusion::tool_number_zero_is_none`, `fusion::schema_version_below_11_is_rejected`.

**dreamcarve (integration, `tests/`)**
- `cli::plaque_svg_exports_two_programs_in_order` (acceptance 18), `cli::plaque_dxf_exports_same_programs_as_svg`.
- `cli::init_dxf_without_units_asks_for_flag`, `cli::init_dxf_layer_and_join_flags`.
- `cli::tools_import_fusion_reports_each_tool`, `cli::tools_import_twice_is_already_present`, `cli::tools_add_copies_into_project_with_new_id`, `cli::library_edit_does_not_change_project`, `cli::tools_export_import_round_trip`, `cli::config_path_prints_locations`, `cli::config_dir_env_override`.
- `cli::config_tool_change_mode_seeds_init`, `cli::one_program_and_programs_per_tool_flags` (acceptance 33).
- `cli::check_reports_order_warnings`, `cli::warnings_print_code_and_message`.
- `cli::posts_lists_six`.
- `cli::canned_cycles_linuxcnc_opt_in` (acceptance 24).

**dc-gcode (integration, `tests/golden.rs`)**
- `golden::pocket_offset_rect`, `golden::pocket_offset_island`, `golden::pocket_raster_0deg`, `golden::pocket_raster_45deg` (`.camj` angle −45; README explains), `golden::pocket_rough_then_rest` (PureCutCNC rough + finish with `stockToLeaveRadial`; compared as per-level sets, union coverage asserted), `golden::drill_simple`, `golden::drill_peck`, `golden::engrave_open_closed`, `golden::profile_ramp` (edge route with `entryStrategy: ramp`; the along-the-path ramp differs from upstream's zigzag, so this case compares the lap moves only and asserts the ramp separately; README says so), `golden::profile_helix_inside`, `golden::profile_lead`, `golden::profile_dogbone`, `golden::order_full_depth_first` and `golden::order_pass_first` (three parts with holes; **compared in order**, not as sets), `golden::post_{linuxcnc,mach3,uccnc,generic}_coaster` (motion equal to phase 1's coaster; dialect lines compared to the verified definitions, not to PureCutCNC's).

## Slices

Ordered; each is one issue and one PR. Dependencies in brackets.

1. **Sequence module and Profile on blocks.** `block.rs`, `sequence.rs`,
   `generate_profile` emitting blocks; O1–O10 tests; phase 1 goldens
   unchanged. Tests: dc-cam sequence group, `profile::one_shape_golden…`.
2. **Format v7: model, migration, validation.** All new types with
   `Entry::Plunge`, `lead: None`, `CornerRelief::None` wired; `ToolKind::Drill`;
   `arc_fit_tolerance_mm`; `use_canned_cycles`; `SpindleDwell::units`.
   Tests: dc-project groups. [none]
3. **Geometry: booleans and circle test.** `boolean.rs` on i_overlay,
   `circle.rs`. Tests: dc-geometry groups.
4. **Pocket, offset strategy.** Regions, rings, wall pass, direction,
   footprint, coverage. Tests: pocket offset/direction/footprint/coverage
   tests. [1, 2, 3]
5. **Pocket, raster strategy.** Tests: pocket raster tests. [4]
6. **Pocket allowance and rest machining.** Tests: allowance and rest
   tests. [4]
7. **Drill.** Generator, `Dwell` move, `DrillHole` records, S15. Tests:
   drill group. [1, 2, 3]
8. **Engrave.** Tests: engrave group. [1, 2]
9. **Entry: helix and zigzag ramp (Pocket).** `entry.rs` port. Tests:
   entry ports. [4]
10. **Entry on Profile: zigzag and helix in the waste domain, On line
    ramp along the path.** Tests: the `entry::*profile*` and
    `entry::on_line_*` tests. [1, 9]
11. **Lead-in and lead-out on Profile.** Tests: lead group. [1, 10]
12. **Corner relief on Pocket and Profile.** Tests: relief group. [4, 10]
13. **Operation-order warnings.** `order.rs`, CLI printing. Tests: order
    group, `cli::check_reports_order_warnings`. [1]
14. **Arc fitting and S12.** Tests: arcfit group, safety S12 tests. [2]
15. **Canned cycles and S11.** Emitter, parser expansion, `reentry_gap_mm`.
    Tests: emit canned tests, safety S11 tests. [7]
16. **S9 and S10 in the safety pass.** Tests: `safety::pocket_cut_outside…`,
    `safety::ramp_steeper…`. [4, 9]
17. **Post-processors: LinuxCNC, Mach3, UCCNC, generic.** Definitions,
    README verification sections, generic refuses `m6`. Tests: post group,
    `cli::posts_lists_six`. [2, 15 for the canned fields]
18. **Multi-tool export flags and user config.** `config.rs`, `--one-program`,
    `--programs-per-tool`, `init` seeding. Tests: emit flag tests, cli
    config tests. [2]
19. **DXF import.** Tests: dxf group. [none]
20. **Tool library crate and CLI.** `dc-library` without Fusion; `tools
    list/add/export/import (DreamCarve JSON)/remove`, `config path`.
    Tests: library group, cli tools tests. [18]
21. **Fusion import.** Tests: fusion group. [20]
22. **Golden corpus, phase 2 cases.** `.camj` cases, notes, capture; order
    comparison mode in `golden.rs`. Tests: golden group. [4–12]
23. **Docs.** Chapters and quick start listed in the functional spec;
    `CONTEXT.md` terms checked (Rest Machining, Entry, Lead, Corner
    Relief added). [all]
24. **Real cut and release.** Cut the plaque, record it, tag `v0.2.0`.

Slices 3, 19 and 20 have no dependencies and can start at once; 1 and 2
unblock everything else.

## Open questions

None. All resolved 2026-10-20; the record is the table in
`functional.md`, "Open questions".
