# Phase 2: Full 2.5D from the command line — Design Spec

**Status:** draft
**Functional spec:** `functional.md`

## Overview

Phase 2 adds three generators to `dc-cam` (Pocket, Drill, Engrave), extends
the Profile generator (entry, leads, corner relief), and puts one shared
sequencing step between every generator and the post-processor. It adds a
DXF importer to `dc-import`, four post-processor definitions to
`dc-project` (where #106 put them), arc fitting and canned cycles to
`dc-gcode`, and a new pure crate `dc-library` for the tool
library and the Fusion importer. The CLI gains `tools`, `config` and the
export flags, and reads the user config directory.

```
SVG / DXF ──dc-import──▶ FeatureDefinitions ──┐
                                              ├─▶ Project (.dcv v8, dc-project)
user edits .dcv ──────────────────────────────┘        │
tool library (dc-library) ─ tools add ─────────────────┘
                                                       ▼
   dc-cam::{profile, pocket, drill, engrave}::generate(project, op)
                         │  Vec<CutBlock> + Vec<Warning>
                         ▼
   dc-cam::sequence::sequence(&SequenceInput) ─▶ Toolpath (project coords, mm, Y-up)
                         │
                         ▼
   dc-cam::order::check(project, toolpaths) ─▶ cut_inside_freed_part, cut_inside_tab_held_part, tool_change_avoidable
                         │
                         ▼
   dc-gcode::emit_programs(project, ops, split, date) ─▶ arc fitting ─▶ Programs + warnings
                         │
                         ▼
   safety pass on the parsed Program (S1–S15; canned cycles expanded first)
```

Everything is millimetres, Y-up, Z-up, stock bottom at Z = 0 (ADR-0001).
The generators are pure; the CLI owns every file, clock and directory.
Where this spec names a PureCutCNC rule that depends on its Y-down frame,
the Y-up form is stated and the Y-down form is noted so the port can be
checked against upstream tests.

**#106 landed first.** "Post-processor as versioned reference plus user
settings" (ADR-0004) took format version 7, so phase 2's format is
version 8. The four new definitions are
`crates/dc-project/src/definitions/<id>/v1.json`, and `use_canned_cycles`
is a project setting beside the post-processor reference. (changed in
slice 2, #117: #106 took version 7 first.) #109 then took version 9 for
`TabSpec::on_holes`. (changed in #109: a part and a hole too small for
tabs needed two operations in the right order.)

## PureCutCNC sources

| DreamCarve | PureCutCNC | Port notes |
| --- | --- | --- |
| `dc-cam::pocket` | `src/engine/toolpaths/pocket.ts` (`buildInsetRegions`, `buildOffsetRegionTree`, `cutOffsetRegionNode` inner-first, `buildPocketParallelSegments`, `transitionToCutEntry`, `generateStepLevels`, `orderClosedContoursGreedy`, `rotateContourToNearestEntry`), `pocketPatterns.ts` (`OPERATION_PATTERN_SUPPORT`, `areaCoverage`) | Only `offset` and `parallel` (named `raster`). No seeded offset, trochoidal, slot feed, engagement feed, corner rounding, wall cleanup, model sections, clamps. Clipper integer paths become `dc-geometry` offsets with cavalier_contours and booleans with i_overlay. Direction rule re-derived (see Pocket algorithm). `pocketAngle` sign re-derived. Islands come from `nest`, not the band resolver. |
| `dc-cam::pocket::rest` | `src/engine/toolpaths/restRegions.ts`, `finishFloorCoverage.ts` (`regionAreaPaths`, `floorCoverage`, `restrictFloorRoot` noise rule) | Rest region = region minus the earlier tool's reachable area. The coverage test is ported; the island-face floor restriction is not (no multi-depth instances in phase 2). |
| `dc-cam::drill` | `src/engine/toolpaths/drilling.ts` (`generateDrillingToolpath`, `emitDrillCycle`, `sortTargetsByNearestNeighbor`, `DrillCycle`), `drillingRetract.test.ts`, `camOperationSmoke.test.ts` drill cases, `toolpaths.test.ts` drill ordering | Circle detection on polylines, not a `circle` feature kind. Chip-break gap is a mm field, not a constant in project units. Dwell is emitted as `G4` when expanded (upstream loses it). Pecks count from the retract plane `R`, as controllers count `G83`/`G73`, not from the stock top (#153). No helical bore, no countersink (phase 3), no clamps. Nearest neighbour starts from the first target in drawing order (D5), not the origin. |
| `dc-cam::engrave` | `src/engine/toolpaths/carving.ts` (`generateFollowLineToolpath`, `buildCarveLevels`), `carving.test.ts` direct-mode cases | Direct mode only; no trochoidal carve, no region masks. Depth is from the stock top and clamped by S1, not to absolute 0. Direction alternation added (D9). |
| `dc-cam::entry` | `src/engine/toolpaths/entry.ts` (`synthesizeEntry`, `findHelixPlacement`, `findLargestClearanceCircle`, `findRampPlacement`, `emitHelix`, `emitRamp`, `pitchFromRampAngle`, `plungeLimitedFeedScale`, `helixAngularDirection`), `entry.test.ts` | Helix sense re-derived for Y-up. Search caps kept (`MAX_HELIX_RADIUS_ATTEMPTS`, 72 angles, 12 endpoint candidates, 20 000 cells). Profile ramp (along the path) is DreamCarve's; PureCutCNC's zigzag ramp serves Pocket. |
| `dc-cam::lead` | `src/engine/toolpaths/xyLead.ts` (`planXyLeadIn`, `planXyLeadOut`, `contourArrivals` corner scoring, `validateLead`, radius and sweep ladders), `xyLead.test.ts`, `xyLeadEdge.test.ts` | The user sets the radius (VCarve); the ladder shrinks from it. Sweep ladder kept. Overcut is DreamCarve's (VCarve). Feed is the cut feed, not the plunge-scaled feed, because the descent is finished before the lead. |
| `dc-cam::relief` | `src/engine/toolpaths/cornerRelief.ts` (`collectReliefCorners`, `reliefExcursionEnd`, `generateCornerReliefPass`, `chooseTBoneEdge`, `mainPathCutsAt`), `cornerRelief.test.ts` | `t_bone` goes on the longer edge (VCarve placement); `longest_edge` variant dropped. Keep-outs dropped. |
| `dc-cam::sequence` | `src/engine/toolpaths/multiFeature.ts` (`perFeatureOperations`, `mergeToolpathResults` nearest), `geometry.ts` (`greedyNearestNeighbor`), `pocket.ts` (`transitionToCutEntry`, `transitionLinksAtDepth`) | New module per D10. Blocks, containment precedence, nearest neighbour, links. `maxLinkDistance = tool diameter` for stay-down links inside cleared material. |
| `dc-cam::order` | `src/engine/operations/camPlan/createCamPlan.ts` (outside-last as a planning rule) | DreamCarve warns instead of planning: `cut_inside_freed_part`, `cut_inside_tab_held_part` (#269), `tool_change_avoidable`. |
| `dc-gcode::arcfit` | `src/engine/gcode/arcFitting.ts` (`fitArcsInMachineMoves`, `splitArc`, `resolveEmittedArc`, `runSurvivesFormatting`, `applyEmittedArcFallback`), `arcReconstruction.ts` (`findArcRunsInPoints`, Kasa fit, endpoint projection, gates), `arcFitting.test.ts` | Runs in project space before the machine transform (no mirror, so handedness is preserved). Tolerance from the project. S12 gates added to the safety pass. |
| `dc-gcode::emit` canned cycles | `src/engine/gcode/postprocessor.ts` lines 301–430 | One cycle word per Drill operation; mixed cycles expand. `G4` dwell on expansion. Safety pass expands cycles (S11). |
| `dc-project/src/definitions/{linuxcnc,mach3,uccnc,generic}/v1.json` | `src/engine/gcode/definitions/*.json` | Data start point only. Every line re-verified against the controller's documentation or source; `G43 H` and `M6` lines removed; `G40 G49 G80` kept where the controller documents them; see "Post-processor definitions". |
| `dc-import::dxf` | `src/import/dxf.ts` (entity set, `$INSUNITS` table, bulge-to-arc, INSERT transform and skip rules, stitching and closing, dedupe) | Parsed by DreamCarve's own group-code reader (ASCII and binary; #124 replaced the `dxf` crate). No Y flip (DXF is Y-up). `ELLIPSE` added. Join tolerance 0.01 mm. Entity skips are warned with counts. |
| `dc-library::fusion` | none (PureCutCNC has no Fusion tool import; `src/toolLibrary.ts` is a bundled list) | New. Written against Autodesk's statements and vendor-published `.tools` files; see "Fusion import". |
| `dc-library::library` | `src/toolLibrary.ts` (`parseToolLibraryFile` validation rules) | Validation rules ported (positive diameter, flutes ≥ 1, stepover in (0, 1], V-bit angle in (0, 180)). Persistence is DreamCarve's. |
| `tests/golden/{pocket-*, drill-*, engrave-*, profile-{ramp,lead,dogbone}, order-*}` | captured via `tools/parity/` | See Test plan. Raster angle and helix sense are mirrored in the `.camj`; each case's notes say how. |

Files in the first column that port logic carry the upstream attribution
header.

## Data model

### dc-project (format version 10)

```rust
pub const CURRENT_VERSION: u32 = 10;  // #106 took 7 (slice 2, #117); #109 took 9; #207 took 10

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
    Helix { diameter_fraction: f64, angle_deg: f64 }, // init: 0.8, 5.0; fraction in (0, 1]; angle (0, 45]
}

pub struct Lead {
    pub radius_mm: f64,          // > 0
    pub sweep_deg: f64,          // init 90; valid (0, 180]
    pub lead_out: bool,          // init true
    pub overcut_mm: f64,         // init 0; ≥ 0
}

pub enum CornerRelief { None, Dogbone, TBone }       // "none" | "dogbone" | "t_bone"

pub struct TabSpec {
    // phase 1 fields unchanged, plus (version 9, #109):
    /// Holes get tabs too: the outlines nested as holes among the
    /// operation's targets. `init`: false; migration: true.
    pub on_holes: bool,
}

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

`Operation::{id, name, enabled, tool, targets, safe_z_mm, depth_mm,
through_cut_allowance_mm}` are accessors over every variant. The project's
post-processor settings gain one user setting:

```rust
pub struct CannedCycles { /* phase 1 fields */ pub reentry_gap_mm: Option<f64> }
pub struct SpindleDwell { /* phase 1 fields */ pub units: DwellUnits }   // Seconds | Milliseconds
/// The sixth project setting beside the post-processor reference (#106):
pub use_canned_cycles: bool,   // migration: false
```

**Migration v7 → v8** (`migrate::v7_to_v8`): add `arc_fit_tolerance_mm:
0.01` to the project (fitting on; a user who wants the phase 1 bytes sets
`null`), `number: null` to every tool, `machining_order:
"full_depth_first"`, `entry: "plunge"`, `lead: null`, `corner_relief:
"none"` to every Profile, and `use_canned_cycles: false` to the
post-processor settings. Each key goes where a version 8 file writes it,
and the migration fails if one is already there. The fixture
`crates/dc-project/tests/fixtures/dcv/v7/sample.dcv` loads and exports
the same program as before (`cli::v7_project_exports_the_same_program`);
with `arc_fit_tolerance_mm` set back to `null` the output is
byte-identical (acceptance 37). Open question 8 in the functional spec
records why this is a migration and not a serde default. (changed in
slice 2, #117: #106 took version 7, so this is `v7_to_v8`, not
`v6_to_v7`.)

**Migration v8 → v9** (`migrate::v8_to_v9`, #109): add `on_holes: true`
at the end of every Profile's non-null `tabs`, so a project whose holes
had tabs keeps them and cuts the same; `tabs: null` and other operation
kinds are untouched, and the migration fails if the key is already there.
The fixture `crates/dc-project/tests/fixtures/dcv/v8/sample.dcv` loads as
the sample project (`migrate::version_8_fixture_loads`,
`migrate::v8_tabs_keep_their_holes_tabbed`). `init` writes `false`: a
hole's slug is waste, and holding it is extra hand work. (changed in
#109: phase 1 tabbed every outline an operation cut, so a part with a
hole too small for tabs took two operations, and in the wrong order the
hole was cut in a loose part.)

**Migration v9 → v10** (`migrate::v9_to_v10`, #207): `Side` gains
`Left` and `Right` (`"left"`, `"right"`), which no version 9 file holds,
so the step is the identity and only `version` changes. The bump is what
ADR-0002 asks for: a version 9 reader refuses a file that uses the new
sides with `NewerVersion` instead of a schema error on an unknown side.
The fixture `crates/dc-project/tests/fixtures/dcv/v9/sample.dcv` loads as
the sample project (`migrate::version_9_fixture_loads`,
`migrate::v9_to_v10_changes_only_the_version`,
`migrate::left_and_right_sides_round_trip`). See "Profile left and right
of an open path".

`DwellUnits` and `reentry_gap_mm` are definition fields, and every
definition field is required, so the bundled `v1.json` files were edited
in place and re-pinned; their emitted bytes do not change
(`definitions/README.md`, "Versions"). `v6_to_v7` gives a version 6
embedded copy `"seconds"` and `null` before comparing it with the bundled
dialect.

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
outside (0, 1]; an `Entry::Helix` angle outside (0, 45]; `Entry::Helix` on a `Side::OnLine`, `Side::Left` or `Side::Right`
Profile (`Problem::HelixNeedsWasteSide`, Left and Right since #207); `use_canned_cycles: true` on a
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
    pub descent_from: Point,         // XY where the descent begins: `entry` for a plunge, the ramp or helix start otherwise
    pub descent_from_z_mm: f64,      // Z the descent starts at (the entry clearance above the stock top or a Pocket's previous pass Z, #214; the previous pass Z of this shape; safe Z for a plunge)
    pub descent: Vec<Move>,          // ramp/helix moves from `descent_from` to entry; empty = straight plunge; a Drill's is one rapid to retract_z
    pub entry: Point,                // XY where the descent ends and cutting begins
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
    pub stepover_mm: f64,            // 0 for Profile; stay-down links ≤ tool_diameter_mm + stepover_mm
    pub feed_mm_min: f64, pub plunge_mm_min: f64,
}
/// The block order, as indices into `blocks`, so a generator can count
/// contours "in cut order" in its warnings before it sequences.
pub fn plan(input: &SequenceInput) -> Result<Vec<usize>, CamError>;
pub fn sequence(input: &SequenceInput) -> Result<Toolpath, CamError>;
// CamError::InvalidBlock { shape, pass, problem: BlockProblem }  (step 1 fails)
// CamError::ContainmentCycle { shape }                           (no shape can be cut first)

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
    pub fn check(project: &Project, generated: &[(&Operation, &Toolpath)]) -> Vec<Warning>;  // cut_inside_freed_part, cut_inside_tab_held_part, tool_change_avoidable
}
```

(changed in slice 1, #127: `CutBlock::descent_from`,
`SequenceInput::stepover_mm`, `plan` and the `Result` returns added; a
library crate never panics, so step 1 of "Sequence" returns an error.)

New `WarningCode`s (params in parentheses; a shape is named by
`instance`, `instance_name` and `profile`): `EntryFallback (op, instance,
instance_name, profile, requested, used, reason)` (`reason`: `no_room` or
`too_many_moves` or `too_small`; once per shape and entry used; changed
in #265: `too_small`), `HelixDiameterReduced
(op, instance, requested_mm, used_mm)` (once per shape, the smallest
diameter cut), `LeadShrunk (op, instance, requested_mm, used_mm,
requested_sweep_deg, used_sweep_deg)` (once per shape, the smallest
radius and sweep cut; slice 11, #180), `LeadDropped (op, instance,
reason)` (`no_room`, `exit`, or `on_line` once per operation with no
instance), `ReliefCornerTooTight (op,
instance, x_mm, y_mm)`, `ReliefCornerNotCut (…)`, `ReliefSkipped (op,
reason)`, `PocketNoRoom (op, instance, instance_name, profile, tool,
diameter_mm)` (tool wider than the region; the tool-centre region does
not depend on the pass, so there is no `pass`), `PocketIslandTooClose
(op, instance, island, other, gap_mm, needed_mm, allowance_mm, tool,
diameter_mm)` (once per island-wall or island-island pair closer than
`needed_mm = diameter + 2 × allowance_mm`; `other` is `wall` or a profile
index; changed in #168: the allowance was ignored),
`DrillTargetNotCircle (op, instance, profile)`, `DrillToolWiderThanHole
(op, instance, hole_mm, tool_mm)`, `DrillPeckDeeperThanHole (op, peck_mm,
depth_mm)` (once per operation: every hole has the same depth),
`DrillRetractBelowStockTop (op, requested_mm, used_mm)` (a negative
`retract_mm` is raised to the stock top), `EngravePathDegenerate (op,
instance, profile)`, `CutInsideFreedPart (op, earlier_op, instance)`,
`CutInsideTabHeldPart (op, earlier_op, instance, held)` (#269),
`ToolChangeAvoidable (tool, ops)`, `RestOfClearsNothing (op, rest_of,
tool)`. All phase 1 codes stay. (changed in slices 4, 7 and 9, #137,
#138, #147: params as built.)

### dc-gcode

```rust
pub enum ProgramSplit { ProjectDefault, OneProgram, ProgramPerTool }   // CLI flags
pub fn emit_programs(project: &Project, ops: &[EmitOp], split: ProgramSplit, date: &str) -> Result<(Vec<Program>, Vec<ExportWarning>), ExportError>;
/// Owns its data, like phase 1's tuple; `From<(Operation, Tool, Toolpath)>` builds one without holes.
pub struct EmitOp { pub op: Operation, pub tool: Tool, pub toolpath: Toolpath, pub holes: Option<Vec<DrillHole>> }

pub mod arcfit {
    pub const ARC_FIT_MAX_SWEEP_DEG: f64 = 90.0;
    pub const ARC_RADIUS_CONSISTENCY_MM: f64 = 0.005;   // GRBL's check, grbl/gcode.c
    pub const ARC_RADIUS_GROSS_MM: f64 = 0.5;
    pub const ARC_RADIUS_RELATIVE: f64 = 0.001;
    pub const MAX_CANDIDATE_POINTS: usize = 128;        // longest run tried at once
    pub const MIN_RUN_CHORDS: usize = 3;
    pub fn fit(moves: &[Move], tolerance_mm: f64) -> Vec<Fitted>;
    pub enum Fitted { Move(Move), Arc(ArcRun) }         // Move: passed through (rapids, plunges, toolpath arcs, unfitted lines)
    pub struct ArcRun { pub arcs: Vec<SubArc>, pub feed_mm_min: f64, pub fallback: Vec<Move> }
}
```

(changed in slice 15, #148: `EmitOp` owns its data instead of borrowing.
Changed in slice 14, #128: the pass-through variant is `Fitted::Move`, not
`Line`, because rapids and toolpath arcs pass through too. Changed in
slice 18, #126: `emit_programs` takes the program date.)

`ExportError` adds `OneProgramNeedsM6 { post }`, `RapidIntoUncut { line,
z_mm, cut_mm }` (S15 on the parsed program), `EntryTooSteep { op, line }` (S10), `OffMaterial {
op, line }` (S9 on the parsed program), and `ArcFitViolation { op, line }`
(S12).

S9 on the parsed program (`dc-gcode::material`) works out each
operation's material from the project, never from the generator. A
Pocket's tool centre stays in its regions, `r + allowance` from their
walls. A Drill's stays in its holes, and an Engrave's on its lines. A
Pocket's corner-relief notches may cut into their corners' relief zones.
A Profile is held to its waste (#177). Inside, the tool centre stays in a
target region, `r` from its walls, as a Pocket with no allowance. Outside,
it stays outside every target region, `r` from every outline. Which side
of an outline an Outside cut or a lap is on is judged at the cut's
middle, not its start. (changed in #231: judged at its start, a T-bone's
return leg, which starts on the wall it slots into, failed on a rotated
part once the program's rounding moved that start up to a quantum into
the part; a cut that does reach into the part still fails the clearance
test.) On line,
it stays on a target's line, at least `d` from every other line of that
line's region. (changed in #199: On line was held to the line only, so a
ring narrower than the tool passed; dc-cam now refuses it, see "Profile
bands".) A cut on an open path stays on that path, at least `d` from
every line of the region whose material holds the path and from every
other open path that region holds, the stretches within `√2·d` of a
point where the two meet set aside. (changed in #210: a cut on an open
path was held to the path only, so a line drawn beside its part's wall
passed; dc-cam now refuses it, see "Profile bands".) A cut on a region's
line, or on an open path it holds, also keeps `d` from the stretches of
that line more than `√2·d` along it from where the cut runs, the
stretches within `√2·d` of where the line meets itself set aside, and on
a closed line only where the part lies between them; an open path drawn
twice is one path. (changed in #221: a cut was never measured against
its own line, so a hand-written hairpin passed; dc-cam now refuses it,
see "Profile bands".) No cut runs on a line of a region whose outline,
offset inward by `r` less the slack, is empty: the cut would remove the
whole part. (changed in #223: a cut round a part smaller than the bit
passed, because it stays on its line and the part has no stretches far
apart along it; dc-cam now refuses it, see "Profile bands".) Left and
Right stay on a target open path offset by `r` to their side, at least
`r` from every open path of the targets and from every path's kept edge
(added in #207, see "Profile left and right of an open path"). The notch rule applies Inside and Outside,
so every Profile cutting move is checked: laps, descents, lead arcs and
notches. One exception is the lap of one outline, a cut at `r` from that
outline on its waste side. It passes even where an outline of another
region comes nearer, because that gouge is the one `ToolpathsOverlap`
warns of. It still keeps `r` from every outline of its own region. A
relief notch that runs within `r` of its outline counts as that lap
here too: a T-bone in a hole's corner beside an island part closer than
the tool starts at the lap's corner and runs away from the island, so
it comes no nearer the island than the lap does (#231).
(changed in #192: the exception covered every other outline, so an
Inside island's lap in a band narrower than the tool passed although
`ToolpathsOverlap` never compares one region's lines; dc-cam now refuses
that band, see "Profile bands".) dc-cam's
domain also keeps entries out of other shapes' kerfs. That is waste too,
so S9 does not check it. (changed in #177: Profile was left to S6 and
dc-cam.) `ExportWarning` adds `ArcFitFallback { op, runs }`,
`CannedCycleExpanded { op, reason }` (the emitter wrote a Drill as plain
moves although `use_canned_cycles` is on) and `UnitsSetByController {
post, units }` (a definition with no units words; the CLI prints it once
per export).

### dc-library

```rust
pub const LIBRARY_VERSION: u32 = 1;
pub struct Library { pub version: u32, pub tools: Vec<LibraryTool> }
pub struct LibraryTool { pub id: LibraryToolId, pub tool: Tool /* id ignored */, pub source: Option<String> }  // source: "fusion:<guid>:<preset>" or None
pub fn load(bytes: &[u8]) -> Result<Library, LibraryError>;
pub fn save(lib: &Library) -> Result<String, LibraryError>;   // stable key order, one tool per block; refuses NaN/inf
pub fn add(lib: &mut Library, tool: Tool, source: Option<String>) -> Result<AddOutcome, LibraryError>;   // Added | AlreadyPresent (same fields)
pub fn replace(lib: &mut Library, tool: Tool, source: Option<String>) -> Result<AddOutcome, LibraryError>; // Replaced when `source` matches, else as `add`
pub fn find(lib: &Library, key: &str) -> Result<&LibraryTool, LibraryError>;  // by id, else unique name
pub fn remove(lib: &mut Library, key: &str) -> Result<LibraryTool, LibraryError>;
pub fn validate_tool(id: &LibraryToolId, tool: &Tool) -> Result<(), LibraryError>;
pub mod fusion {
    pub const MIN_FUSION_VERSION: u64 = 11;
    pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;   // unpacked document cap (zip bombs)
    pub fn is_fusion_library(bytes: &[u8]) -> bool;          // zip signature, or a JSON object with a `data` array
    pub fn import(bytes: &[u8]) -> Result<FusionImport, LibraryError>;    // .tools (zip) or .json
    pub struct FusionImport { pub tools: Vec<ImportedTool>, pub skipped: Vec<Skipped> }
    pub struct ImportedTool { pub tool: Tool, pub source: String, pub notes: Vec<ImportNote> }
    pub enum ImportNote { NoPresets, FeedUnset, PlungeUnset, RpmUnset, FlutesUnset, StepOverClamped { step_over_mm, diameter_mm } }
    pub struct Skipped { pub description: String, pub reason: SkipReason }
    pub enum SkipReason {
        UnsupportedType(String), UnknownType(String), MissingType,
        UnknownUnit(Option<String>), NoDiameter, NoTaperAngle,
        Invalid { field: String, message: String },
    }
}
// LibraryError (Fusion): NotAFusionLibrary, MissingVersion, FusionVersion { found }, FusionArchive { message }
```

(changed in slice 20, #136: `save`, `add` and `replace` return `Result`,
because an invalid tool is refused. Changed in slice 21, #152:
`ImportedTool` carries the notes the report prints, and more skip
reasons, so one bad item never fails the whole file.)

### User config (CLI only)

`<config dir>/dreamcarve/config.toml`:

```toml
[export]
tool_change_mode = "separate_programs"   # or "m6"; applied by `init` and `--post`
```

`<config dir>/dreamcarve/tools.json` is the library. `config.rs` finds
`<config dir>` with std only: `DREAMCARVE_CONFIG_DIR` first (tests use
this); else `$HOME/Library/Application Support/dreamcarve` on macOS,
`%APPDATA%\dreamcarve` on Windows, and `$XDG_CONFIG_HOME/dreamcarve`,
falling back to `$HOME/.config/dreamcarve`, on Linux and everything else.
An unset or empty `HOME` or `APPDATA` is an error naming the variable.
Only `init` and `export --post` read the config, so a project exports the
same on every machine. A missing config file means the defaults; a
malformed one, or an unknown key, is an error naming the file, line and
column. (changed in slice 18, #126: `directories` pulls in an MPL-2.0
crate, and std fits.)

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
   warns `PocketNoRoom` and the region is skipped. Joins are round on the
   outer and on islands: `dc_geometry::offset` (cavalier_contours) has
   round joins only. An inward offset keeps the outer's convex corners
   sharp either way, and at a reflex corner a round join keeps the tool
   centre exactly `r` from the corner. When the offset islands lie apart
   inside the offset outer, the region is built from the offsets with
   `nest`, so arcs stay arcs; otherwise the boolean difference gives a
   polygonal result. (changed in slice 4, #137: PureCutCNC mitres the
   outer; cavalier_contours cannot.)
3. **Rest region** (`rest_of` set): let `R` be the earlier Pocket's tool
   radius and `A` its allowance, read from the project (S14: never its
   toolpath). Its reachable area is `offset(inset_region(region, R + A),
   +R)` (what its tool of radius `R` swept along its tool-centre region
   covers; PureCutCNC `generateAreaRestRegionDrafts`). What it left is
   `inset_region(region, allowance) − reachable` (this operation's own
   allowance), less pieces thinner than twice the flatten tolerance
   (area under `tolerance × perimeter`, the noise rule below). The
   tool-centre region of step 2 is then cut down to the centres within
   `r` of what is left: `tc ∩ offset(left, +r)`. The rest region it
   clears is that grown by `r`, which lies inside `region` and overlaps
   the already-cut area by up to the small tool's diameter, so the wall
   band and corner wedges are cleared with no ridge at their edge. (The
   leftover's edge against the cut area is not a wall: insetting the rest
   region itself by `r` would leave nothing of a band narrower than the
   tool.) If nothing is left that this tool reaches, in every region with
   room for it, warn `RestOfClearsNothing` and emit nothing. Because the
   reference's `allowance_mm` is in the inset, a rough pocket with
   allowance 0.5 leaves a 0.5 mm band the rest pass removes. A chain
   (rest of a rest) needs no special case: the second rest's reference
   reaches exactly what its tool can, which is what the first two
   operations cleared. A root ring of the rest tool-centre region that
   lies outside `left` and at least `r − tolerance` from it (the band's
   inner edge, which is `left` grown by `r`) sweeps only cleared area and
   is not cut, in the offset walk and in the raster wall pass; the region
   itself, its entry domain and every ring's `cleared` area are
   unchanged. (changed in #167: that ring was cut in air, 276 mm a pass
   on acceptance 22.)
4. **Passes.** As Profile step 5 (`passes::depths`), with `step =
   op.step_down_mm.or(tool.step_down_mm)` and S1.
5. **Stepover** `s = diameter × step_over_fraction` (operation, else tool;
   validation guarantees (0, 1]).
6. **Offset strategy.** Build the ring tree once per tool-centre region:
   `children(node) = inset_region(node, s)`, recursively, until empty;
   an inset whose area is under the flatten tolerance times its
   perimeter (a piece thinner than twice the tolerance, rest machining's
   noise rule) is not a node. (changed in #235: a nested circle 5.0006
   step-overs in radius kept a sixth ring 0.0002 mm in radius, three
   arcs whose ends round to one point, and S6 refused the program.)
   Every node's outer and islands are rings. Emit blocks per pass,
   innermost first (PureCutCNC `inner-first`): for each root region, a
   depth-first post-order walk emits the leaf rings, then their parents,
   ending with the root's rings, which are the wall and island walls.
   With `wall_pass: first` the root's rings are emitted before the walk;
   with `none` the root's rings are omitted and the first child becomes
   the outermost ring (the pocket is undersize by `s`, as VCarve's "No
   Profile Pass" warns; a region too small for a second ring is then not
   cut). **Residual rings:** when `s > r + tolerance`, each node also gets
   residual leaf children, one per piece of `inset_region(node, r + tol)`
   less its children (noise dropped) grown by `r + tol` (`rings::residual_regions`),
   pieces under `tolerance × perimeter` dropped as noise. A ring sweeps
   `r` either side, so those are the points neither the node's rings nor
   its children's subtrees reach; every one lies under `s ≤ 2r` from the
   node's rings, so one ring round each piece reaches all of it. Each
   residual ring is cut from inside the lap of its node's ring
   (`rings::Splice`): the point of the node's outer or a hole nearest a
   vertex of the piece's ring (among vertices and arc points, so a
   straight edge offers its corners; ties to the outer, then the earlier
   hole and vertex), and the piece's point nearest that. When the lap
   reaches that point it runs straight to the piece's point, round the
   residual ring (wound by the direction rule, as an outer), and straight
   back, at the pass Z and the feed, then goes on; a piece reached from
   the lap's start is cut when the lap closes there. The ring has cleared
   `r` about that point and the piece is at least `r + tol` from the
   ring, so the link runs out through cleared material. A piece with a
   hole, a link longer than one diameter plus `s` (the stay-down limit)
   or one that leaves the node's region (checked exactly against its
   edges, arcs as arcs, and sampled every 0.1 mm), and a piece whose
   ring a rest operation does not cut (#167), is instead an ordinary
   ring in the walk (visited nearest-first with the children, `cleared`
   its own region). With `none` the root's residual rings are omitted
   with the root's rings. At `s ≤ r` there are none, so 40 % output and
   the goldens are unchanged. (changed in #258: above half the diameter
   the innermost ring could lie up to `s` from the middle of its region
   and reach only `r`, a 4.3 mm pin standing in a 17 mm circle at
   step-over 1.0, and above `(1 + √2/2)·r` each ring's right-angle
   corners were more than `r` from the ring inside, 2.2 mm² a corner a
   level on the 100 × 50 rect; PureCutCNC's contour pocket has the same
   gap, and its trochoidal `appendResidualCoreGuides` adds a ring to
   leaves only. Raster has no such gap: its lines are `s ≤ 2r` apart and
   the wall pass cuts within `r` of the edge.) (changed in #267: as rings
   of their own, the corner pieces lay far apart and each cost a lift and
   an entry, 13 lifts per pass on the 100 × 50 rect at step-over 1.0
   against 1 at 0.8; cut from the lap, 1 per pass with the wall pass last
   and 2 with it first, as at 0.8. A leaf's centre piece is now cut after
   the leaf's ring, from it, rather than before; the corner pieces of the
   root are cut from the wall pass.) The tree is built and walked with
   an explicit stack, capped at `MAX_RINGS` = 10 000 nodes per tool-centre
   region (`InvalidStepOver` above it), residual nodes included. At each
   node the children are visited nearest-first
   (upstream `orderNodesGreedy`), then the node's own rings. Ring blocks
   of one pass share a `group` so `sequence` keeps the inner-first order.
   Each ring's `cleared` area is the ring's node region in tool-centre
   terms, not grown by `r`, so a stay-down link's centre line stays in the
   tool-centre region and the tool stays in the pocket. The link from a
   ring to the next one outward still stays down under the `sequence`
   link rule; a link to a ring of another root region, or across an
   island, retracts. Each ring starts at its point nearest the previous
   ring's end (`rotateContourToNearestEntry`) among its vertices and
   along its arcs, the arc split there, computed once from pass 0's walk
   and reused for every pass; a straight edge offers only its corners.
   (changed in slice 4, #137: grown by `r`, a link could reach `r` past
   the wall of a concave pocket.) (changed in #196: upstream takes the
   nearest vertex of a Clipper polygon, whose arcs are sampled densely, so
   on an arc that is in effect the nearest point; DreamCarve keeps arcs,
   a circle has two vertices, and the link onto the first ring round the
   island of `pocket-offset-island` was 7.6 mm where upstream's is
   2.54 mm.)
7. **Raster strategy.** Rotate the tool-centre region by `−angle_deg`
   about the origin (so lines at `angle` become horizontal), scan
   horizontal lines from `max_y − s/2` down to `min_y + s/2` in steps of
   `s` (PureCutCNC's scan from its Y-down `minY`, which is the top in
   Y-up; one line in a region narrower than `s`; one more line at
   `min_y + s/2` when the scan stops more than `s` short of the far edge,
   where upstream's last line can sit up to `1.5·s` away), intersect
   each with the region (even-odd over outer and islands; horizontal
   edges ignored, crossing rule `a.y ≤ y < b.y` or `b.y ≤ y < a.y`),
   rotate the segments back by `+angle_deg`. Lines alternate direction
   in scan order; each segment of a split line (around an island) is its
   own block. (changed in #195: the scan ran from `min_y + s/2`, upstream's
   expression kept in the Y-up frame, so it started at the bottom and its
   lines sat up to `s/2` from upstream's.) Order: the segments of one
   tool-centre region are cut nearest-first (upstream
   `orderOpenSegmentsGreedy`), from the end of the previous path: the
   next segment is the nearest one a stay-down link reaches (the link
   rule below), entered at its nearer end, so a segment may run either
   way; when none is, the tool lifts, to the nearest end that no
   stay-down link reaches from a segment still to cut (a dead end), else
   to the nearest end. Ties go by scan order, start before end. When scan
   order would lift fewer times, it is kept. (changed in #150: scan order
   lifted once per line an island splits, 11 times per pass on the
   island test pocket; nearest-first zigzags down one side of the island
   before it crosses, 4 lifts per pass round a 20 mm square island.
   Upstream stays down on any link up to a diameter long, even one that
   cuts into an island; DreamCarve keeps its link rule, so next to a
   curved island side, where every straight link between line ends
   enters the island, each such end is a dead end and no order lifts
   less than scan order.) Edge links: with a wall pass (`last` or
   `first`), two segment ends that no straight stay-down link joins but
   that both lie on one ring of the tool-centre region, its outer or an
   island's (within
   the flattening tolerance, as the segments end on its chords) are
   joined along that ring: to the ring, the shorter way round it with its
   arcs kept, and off it, at the pass Z and the feed, when that way is no
   longer than one diameter plus `s`. The ordering counts it as a
   stay-down link; the segments and their edge links are one block,
   whose `cleared` area is the segments' strips and the band the edge
   links sweep (grown by `r`, cut to the region), so `sequence` sees no
   link inside it. The tool centre on the ring touches the wall or
   island exactly, so the block cuts only the band that ring's wall pass
   cuts anyway (or, for a ring a rest operation skips, cleared air), and S9
   holds by construction. Without a wall pass that band is not the
   operation's to cut, and no edge link is made. (changed in #202: round
   a curved island side every line end was a dead end, 11 lifts per pass
   on the 20 mm circle island test pocket; following the ring, 1 lift per
   pass with the wall pass last and 2 with it first, and round the 20 mm
   square island, whose ring has round corners, 2, from 4. Upstream's
   straight link there gouges the island by up to 0.38 mm.) (changed in
   #218: only island rings were followed, but the outer ring bulges into
   the region wherever the wall curves inward, round a reflex corner (an
   arc of radius `r`) or along an inward arc, and a straight link there
   leaves the region; with the outer ring offered too, 0 lifts per pass
   from 1 on a 100 × 50 L, and 1 from 6 on the 100 × 50 rect with a
   radius-15 half-disc bay in its top wall, wall pass last or first.
   Straight links win wherever valid, so convex outlines and the
   goldens do not change.) Links:
   `sequence` stays down when the straight
   link between two blocks lies inside the tool-centre region (no island
   between) and is shorter than one diameter plus `s`, else retracts. The wall pass rings (root rings of step 6 with no
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
   placed once per ring or line (`entry::plan`) in the tool-centre region
   that ring or line's pass cuts: the root region, or with `wall_pass:
   none` the root's children, so a descent never starts over the uncut
   band. Each pass then builds its descent (`EntryPlan::descend`), from
   `ENTRY_CLEARANCE_MM` above the stock top on pass 0 and that far above
   the previous pass Z after that (PureCutCNC `levelEntryStartZ`: every
   pass clears the whole tool-centre region, so the pass before emptied
   it down to its Z). (changed in #214: pass 0 started at safe Z, later
   passes at the previous pass Z.)
10. **Corner relief** (`relief` module): after all passes, with
    `cleared = region inset by allowance` and `walls = tool-centre
    region's root rings`, collect corners and emit the notch blocks in
    their own group at the end, stepping down at the tool's step-down.
11. **Blocks → `sequence`** with containment pairs from `nest` depth.
12. **Footprint check** (S9): `footprint(toolpath, r)` must lie inside
    `region` inset by `allowance_mm` (offset by `+EPS_MM`) and outside
    every island grown by it; else `CamError::OffMaterial`. Descents are
    checked on their tool centres against the tool-centre regions, which
    is exact for a round tool; sweeping a helix's chords through
    `footprint` is too slow (#158). A rest operation is checked against
    its own region the same way (S14).
    Stock bounds as Profile step 8.

Coverage check (acceptance 19, 22): `union(footprint per pass) ⊇
inset_region(region, r)` within 0.05 mm, computed with i_overlay; the
difference's area must be below `0.05 × perimeter`. PureCutCNC's
`restrictFloorRoot` noise rule (`max(4 units, reach × 1e-3)`) becomes this
tolerance.

### Drill (dc-cam::drill)

1. Resolve targets. For each closed profile, **circle test**: let `c` be
   the centroid of the vertices; the profile is circular if every vertex,
   every arc's midpoint and every line segment's midpoint (so a square is
   not taken for a circle) lies within `flatten_tolerance_mm` of the
   circle of radius `mean(|v − c|)` about `c`, and (for polylines with
   arcs) every arc's centre is within the tolerance of `c`. Non-circular
   or open profiles warn `DrillTargetNotCircle` and are skipped. Hole
   diameter `2 × mean radius` is kept for warnings: tool wider than the
   hole warns `DrillToolWiderThanHole`.
2. `top = thickness`, `bottom = thickness − depth` (S1 with allowance),
   `retract_z = min(safe_z, top + max(retract_mm, 0))` (a negative
   `retract_mm` warns `DrillRetractBelowStockTop`; validation already
   rejects it, but the generator does not trust that), `peck =
   peck_depth_mm.or(tool.step_down_mm)` (validation guarantees one),
   `chip gap` from `retract`. Peck levels come from `passes::pass_z_mm`
   measured from `retract_z` rather than the top (see step 4); a peck
   that reaches `bottom` from `retract_z` in one plunge warns
   `DrillPeckDeeperThanHole` once per operation. A non-positive gap or a
   negative dwell is `CamError::InvalidDrillSetting`. Only the plunge rate
   is required by the generator.
3. Order: nearest neighbour from the first circle in drawing order, ties
   by drawing order (D5).
4. Per hole, one block whose `descent` is one `Rapid` straight down to
   `retract_z` (above the stock), and whose `cuts` are:
   - Simple: `Linear` down to `bottom` at plunge; (dwell) `Dwell(s)`
     pseudo-move; `Rapid` up to safe Z.
   - Peck, full retract: for `z_k = max(bottom, retract_z − k·peck)`
     (from `R`, as a controller's `G83`/`G73` counts; the first peck
     removes `peck − (retract_z − top)` of material, never more than a
     peck; changed in #153, was `top − k·peck`): `Linear`
     down to `z_k` at plunge, (dwell), `Rapid` up to `retract_z`, then
     `Rapid` down to `min(z_k + PECK_REENTRY_MM, retract_z)`
     (`PECK_REENTRY_MM` = 0.5 mm, a named constant: the tool re-enters
     the hole by rapid only through air it has already cut) and the next
     `Linear` continues from there, until `z_k == bottom`; final `Rapid`
     to safe Z. A controller's `G83` re-enters by its own documented gap;
     S11 compares with that gap as the tolerance (see "Canned cycles").
   - Chip break: as peck but each retract is `Rapid` up by `gap_mm` only,
     capped at `retract_z`, and the next `Linear` resumes from there.
   The block's `descent_from` and `entry` are the centre; `sequence`
   rapids to it at safe Z, then the block's descent takes the tool down
   to `retract_z` by rapid. In `sequence` an empty descent means a
   straight plunge from safe Z, so the rapid is the descent. Each hole
   ends with a rapid to safe Z, which is `G98`'s return to the initial Z,
   so S11's expansion of `G81`–`G83` matches the toolpath move for move
   and acceptance 23's "one retract to safe Z" holds. Between pecks a
   full retract still goes to `retract_z` (`R`). Between holes the link
   is at safe Z. (changed in slice 7, #138: the spec had an empty
   descent and a last rapid to `retract_z`.)
5. `Move` gains `MoveKind::Dwell { seconds: f64 }` (no motion; emitted as
   the definition's dwell command in its `units`, `G4 P0.500` for
   seconds and `G4 P500` for milliseconds; the safety parser treats it as
   no motion, and S6 matches each toolpath dwell to a `G4` of the same
   seconds between the surrounding motion lines). The spindle-on dwell
   reads the same `units`.
6. `DrillHole` records per hole for the emitter, with `cycle` = `G81`
   (simple, no dwell), `G82` (simple + dwell), `G83` (peck full, no
   dwell), `G73` (chip break, no dwell), else `Expanded`.
7. S15: every `Rapid` that descends ends at or above the stock top, or
   no deeper than a cut has reached at that XY. `assert_safe` checks it
   on the toolpath; the safety pass checks it inside a Drill span and
   fails with `ExportError::RapidIntoUncut`. Every other operation has
   S2, which since #214 also lets a rapid straight down from safe Z stop
   over a ramp or helix's start, and since #227 over a straight plunge.

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
5. No D6 `ToolpathsOverlap`, for open or closed paths. (changed in #206:
   recorded when open paths joined D6 for On line Profile. Engrave cuts
   every line as drawn at one depth; lines that cross or meet, as in
   lettering, a hatch or a border, are the design, and a line has no kept
   side for another's cut to gouge. O8 does apply to an Engrave's open
   paths.)

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
- **Descent start Z** (#214): a ramp or helix that would start at safe
  Z starts `ENTRY_CLEARANCE_MM` (1 mm) above the stock top instead
  (`entry::descent_start_z_mm`, capped at safe Z): a Pocket's and a
  Profile's first pass, and a Profile helix that restarts because the
  pass before cut no bore. A Pocket's later pass starts the clearance
  above the previous pass Z; a Profile's later pass still starts at the
  previous pass Z, where the tool already is. The sequencer reaches the
  start by a rapid straight down (Sequence, step 4). PureCutCNC's
  clearance is the project's `operationClearanceZ`, its safe Z
  clearance; DreamCarve's is a constant, 1 mm, a Drill's usual
  `retract_mm`, until users ask for a setting. (changed in #214: a
  descent started at safe Z, so a 15° helix spiralled about 3.5 turns
  through the air at the feed rate before it reached the wood, half of
  the plaque's first program; VCarve and Fusion rapid down to a
  clearance above the material first, as the Drill already did.) A
  straight plunge (`entry: plunge`, a fallback plunge, an Engrave pass, a
  corner-relief notch, a lap that starts on a tab) is reached by the same
  rapid down and plunges at the plunge rate only from there (#227).
- **Pitch** `π · d · tan(angle)` for path diameter `d`. **Feed scale**
  `min(1, plunge / (feed · sin(slope)))` on every descending move (S10).
- **Helix placement:** largest clearance circle (pole of inaccessibility,
  quadtree best-first, cached per region); start radius `min(D·f/2, D/2,
  circle_r − safety)`; try the tangent placement (72 angles; the helix
  touches the target point) then the reachable placement (quadtree search
  nearest the target, 12 endpoint tries over 24 angles); halve the radius
  up to 4 times. A target up to the flatten tolerance outside the
  flattened domain counts as on it: the reachable placement's handoff
  and the ramp's chord may start there (#213: a raster Pocket's wall
  ring starts on a round corner of the tool-centre region, on the arc,
  outside the chords the domain holds for it, and neither helix nor ramp
  was placed). Only the handoff's piece that reaches the target may lie
  outside the chords, and by no more than the target does; the rest keeps
  to them (changed in #272: the slack held along the whole handoff let
  one to a raster start on an island's chord run 0.005 mm inside the
  chords, 0.015 mm inside the island's tool-centre circle, past the
  flatten tolerance S9 allows, and S9 refused it). Success below the
  requested diameter warns
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
  target. A final run that would fall less than `RAMP_MIN_LAST_DROP_MM`
  (0.05 mm) shares the last two runs' depth instead, each at `angle`, the
  first cut short and the second back to where it began; a one-run ramp
  that shallow runs the whole run. (changed in #234: a depth a sliver
  over a whole number of runs left a last run of micrometres, written
  level at the plunge-limited feed, which S3 refuses.)
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
  if that pass cut none. A contour cut without a lead whose passes below
  the tab top would plunge at its start (the start under a tab span, or
  so near one that the entry has no room in the domain less the tab
  keep-outs, or, On line, before the first tab) is re-seamed: the
  candidates are the middles of the stretches between tab spans
  (`PathTabs::gaps_mm`), longest first, then nearest the start, and the
  first where no pass below the tab top plunges wins; the original start
  is kept when it already fits and when no candidate does (then those
  passes plunge with `EntryFallback`, `no_room`). The seam is chosen once
  per path and serves every pass; the tab layout moves with it
  (`PathTabs::reseamed`), so tabs stay at their distances from the
  original start. A path with a lead keeps the lead's seam. (changed in
  #176: a start under or just before a tab plunged on every pass below
  the tab top, as phase 1, though most of the contour had room for the
  entry. The middle of a gap, not its start, so a wall entry has the
  same room either way along the contour.)
- **On line ramp:** an On line Profile removes only its own kerf, so the
  ramp runs along the cut path itself: from the start vertex, forward
  along the path for `run = min(3·D, L)` and back, descending at `angle`
  on each leg until pass Z is reached, the final partial leg clamped (a
  final leg that would fall less than `RAMP_MIN_LAST_DROP_MM` shares the
  last two legs' depth, each at `angle`, the first cut short and the
  second back to where it began, as the Pocket ramp; a one-leg ramp that
  shallow runs the whole leg; changed in #234, and in #273 from a whole
  shallower last leg, whose chords along an arc fell under an inch
  program's last place); arcs are split into chords within the flatten
  tolerance while Z changes. A descending piece that would fall less
  than one unit in the program's last place (`EntryTool::quantum_mm`) or
  run less than √2 units in XY is merged into its neighbour within the
  leg (the vertex between them dropped, the leg's ends kept), so its ends
  round apart and keep their slope; when a leg of one piece is still that
  short, or a dropped vertex lies more than a unit off the merged move,
  the ramp falls back to a plunge, reason `too_small` (changed in #273: a
  leg cut short 0.0055 mm past a chord vertex ended in a piece falling
  0.0014 mm, written level at the plunge-limited feed in inch, which S3
  refuses). The pass then cuts the full path from the start vertex, so
  the ramped stretch is cut again at full depth (VCarve's Zig Zag ramp).
  On an open path shorter than one leg the ramp reverses at the path
  end. The feed scale of S10 applies to every leg.
- **On line Profile of an open path** (#175): Profile step 1 keeps open
  paths for `Side::OnLine` (Inside and Outside still skip them with
  `OpenPathSkipped`, an open path having no side; one of no length
  skips with `reason: no_length`). The path is not nested: no band
  check, no containment pair, no waste domain. Conventional cuts it
  from the first vertex to the last, climb from the last to the first
  (VCarve: "Conventional starts cutting at the Start point Climb cutting
  starts cutting at the end point"). (changed in #242: #175 had climb
  start at the first vertex, the opposite of VCarve's rule, which is the
  reference for option behaviour; tabs stay measured from the first
  vertex, so they do not move.) Each pass is one block from the
  start to the far end at pass Z, `frees` false; its descent starts at
  the previous pass Z, so the sequencer retracts, rapids back, and
  plunges down the slot to it before the ramp (`along_path`, which
  reverses at the far end of a short path). Tabs: `Tabs::layout_open`,
  centres from the first vertex as drawn whichever end the cut starts
  at, spans cut short at the ends instead of wrapping; a tab over the
  start of the cut plunges with `EntryFallback` (the start of an open
  path cannot move). Leads and corner relief: none, as every On line
  Profile (`LeadDropped (on_line)`, `ReliefSkipped`). S9 holds a cut on
  an open target to that path, as an Engrave's. Validation's
  `OpenProfile` is not reported for a definition an enabled On line
  Profile targets. (changed in #175: Profile skipped every open path, so
  acceptance 26's 100 mm line could not be cut On line. VCarve's Profile
  cuts open vectors On, with ramps and tabs and without leads, and an
  On line cut has the same meaning on an open path as on a closed one.
  Engrave keeps open paths cut without an entry or tabs, and
  `alternate_direction`.)
- **Fallback chain:** helix → ramp → plunge, each step warning
  `EntryFallback` with `requested` and `used`. A plunge is always
  possible. A placed helix whose chords the program cannot write is not
  cut (`entry::helix_writable`, reason `too_small`): each of its
  descending chords must fall at least one unit in the program's last
  place (`EntryTool::quantum_mm`, from the project's decimal places in
  its display units, `DecimalPlaces::quantum_mm`, the same unit the
  safety pass compares within) and run at least √2 units in XY, so its
  ends round to two Z values and two XY points. (changed in #265: a
  0.05 helix of a 0.55 mm bit, 0.0275 mm across, had chords falling
  0.00045 mm over 0.0018 mm, written level at the plunge-limited feed,
  which S3 refuses. Fewer, longer chords per turn, as the issue
  proposed, would have changed every helix of a small bit to keep a
  fixed 0.05 mm least drop; a helix whose chords the program cannot
  write is not a helix, and one that small removes nothing a ramp would
  not. The rule needs the program's unit, which is why the entry reads
  the post-processor's decimal places; a constant for the coarsest
  program allowed, 0.0254 mm, would refuse a 2.5 mm helix at 5°.) An
  On line ramp follows the same rule (On line ramp, above; #273).
  `ramp` is the default for new projects (`init`, the chapter
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

Slice 11 (#180) decisions:

- **Tabs.** A lead is planned once per path, first in the domain less
  the tab keep-outs; one that fits there fits the whole domain, so it
  serves every pass. Failing that, a lead in the whole domain serves the
  passes above the tab top and `LeadDropped (no_room)` is warned for the
  rest. The tab layout is moved with the seam (`PathTabs::reseamed`), so
  tabs stay at their distances from the contour's own start; the lap and
  the overcut rise over them, the arcs never do.
- **Travel.** Blocks are ordered after they are built, so "the previous
  cut's exit" is the previous path's in drawing order (its lead-out end);
  the first path ranks without travel.
- **Passes.** Each pass starts at the staging point and ends off the
  contour, so the sequencer's "different entry" link applies: retract,
  rapid, and descend at the staging point, through the column the pass
  above cut there.
- **`LeadShrunk`** warns when the radius or the sweep is reduced, with
  `requested_sweep_deg` and `used_sweep_deg` beside the radii, once per
  shape with the smallest cut.
- **On line:** no lead; `LeadDropped (op, reason: on_line)` once per
  operation (not a validation error: ignoring the lead is safe, and a
  project switched to On line still exports).
- **No length budget** (upstream's 2.5·D): the user sets the size. No
  overlap search for the exit (upstream's 0–1·D): the overcut is the
  user's. "Inside domain" near the join allows the flatten tolerance,
  since the domain's arcs are chords and a curved wall's tool path lies
  up to that far outside them. A lead arc whose samples leave its domain
  is `CamError::OffMaterial` (S9), as a descent's.

### Tabs per outline (dc-cam::tabs)

Tabs stay an operation setting (`TabSpec`; placed tabs are #105, with the
GUI), but each tool-centre path gets its own layout, `Tabs::fit`:

1. A path cut for a hole (`Role::Hole`: nested as a hole among the
   operation's targets, at any depth) gets no tabs unless `on_holes`.
   Its last pass through the stock frees the slug. Islands (depth 2) are
   parts and keep their tabs; open paths are never holes.
2. Every other path lays the placement as before (`count` from `L/(2n)`,
   `at_distances` modulo `L`). When the merged spans cover more than half
   of `L` (the `TabsCoverMostOfContour` threshold, `> L/2 + EPS_MM`), the
   part or hole would not cut free, so the path gets the most tabs that
   cover at most half: `count` becomes the largest `n' < n` with
   `n' (w + d) ≤ L/2` (evenly spaced, so never overlapping), and
   `at_distances` keeps its longest prefix within half. A part's outline
   (`Role::Outer`, `Least::One`) never drops below one tab: cut fully
   free, a small part can be thrown by the bit, which is worse than an
   over-held part, so if even one covers more than half that one is cut
   and the warning says the part may need cutting free by hand. A hole
   (with `on_holes`) or an open path (`Least::Zero`) may reach none.
   `TabsCoverMostOfContour` names the shape with the tabs asked
   (`tab_count`) and cut (`cut_count`), and the width at which all would
   fit. (changed in #109 review: parts first dropped to none too, which
   left a part that asked for tabs loose on the through cut.)
3. `order::check` counts an outline a tabbed Profile through the stock
   cuts with no tab (its holes, or a hole too short for a tab) as freed
   (`profile::untabbed_outlines`).

PureCutCNC sizes auto-tabs per feature (`autoTabs.ts`, a 15 % free-path
rule) as a command that writes placed tabs; that port waits for #105.
VCarve applies tabs per vector and lets the user leave vectors out, which
`on_holes` approximates without placed tabs. (changed in #109: one tab
setting applied to every outline, holes included, and a too-short
outline kept tabs covering most of it, warned but cut as set; a star and
its 7 mm hole took two operations.)

### Profile bands (dc-cam::profile)

Phase 1's Profile step 3 offsets each contour on its own. Under Inside
the band between a region's outline and its islands, or between two
islands, is waste, and the material beyond every one of those lines is
kept. A tool-centre path of one line that comes closer than `r` (less
`EPS_MM`) to another line of its own region would cut into that kept
material: in a band narrower than the tool, an island's lap runs past the
outline into the part. `generate_profile` refuses it with
`CamError::BandNarrowerThanTool { op, shape, other, band_mm, diameter_mm
}`, the two shapes in drawing order and `band_mm` the narrowest distance
between their lines. The rule with `ToolTooLarge`:

- A contour with no tool-centre path at all (the tool fits nowhere along
  it) warns `ToolTooLarge` and is skipped, as in phase 1: nothing is cut
  there, so nothing is gouged.
- A path that exists and comes within `r` of another line of its region
  is an error, never a skip. That includes an island whose outline was
  skipped as `ToolTooLarge`: the island's lap would still run outside the
  outline.
- Outside never meets the rule: the outline's path runs outside the
  region and every hole's inside its hole, so each is `r` from every
  other line of the region. A hole close to its outline is a thin wall,
  not a gouge. Paths in different regions stay D6's `ToolpathsOverlap`
  warning; under Inside, once this check passes, they never come within
  `r` of another region's line.

(changed in #192: an Inside band narrower than the tool gouged the part
with no warning, because the overlap check never compares one region's
lines and S9's lap rule passed the lap. A cut that gouges kept material
is an error, not a warning: D6 warns only where two separate shapes were
drawn too close, which the user can see and accept.)

On line, the band between two lines of one region is the part itself: a
ring between an outline and its hole, or a web between two holes. The
tool centre follows each line and the bit cuts `r` of the band beside
it, so the two cuts together cut through a band narrower than the tool,
even where neither reaches the other line. `generate_profile` refuses an
On line path that comes closer than `d` (less `EPS_MM`) to another line
of its own region with the same `BandNarrowerThanTool`. A band exactly
`d` wide is cut. On line always has a path, so `ToolTooLarge` never
applies. Lines of different regions stay D6's `ToolpathsOverlap` at `r`:
the gap between two parts is waste. S9 holds an On line Profile's moves
to the same rule, so a hand-written program that cuts both lines of a
narrow ring fails `OffMaterial`.

(changed in #199: an On line ring closer than `r` to its hole was cut
through with no warning, because the overlap check never compares one
region's lines and S9 checked only that the tool stays on a line. The
issue proposed D6's warning. It is an error instead, consistent with
#192: the part loses material it keeps, which is the harm #192 refused
for Inside. No Profile use needs it: a double line drawn for its own sake
is Engrave's job, and Engrave has no such rule. The threshold is the
diameter, not the issue's radius, because a ring between `r` and `d` wide
is cut through too, by the two cuts together.)

An open path an On line Profile cuts (#175) takes part in D6 by the same
On line rule: its line is its tool-centre path, and it warns
`ToolpathsOverlap` with another shape when either comes closer than `r`
(less `EPS_MM`) to the other's line, crossing included (`gap_mm` 0). It
belongs to the region whose material holds all of it (every vertex and
every segment's middle, `Region::contains_point`, lines included) and is
not compared with that region's lines, as a hole is not; held by no
region (in the waste, in a hole's slug, or across a line) it is a group
of its own, compared with every other closed shape and open path, and
with itself (#224). The pairs keep cut order.

An open path held by no region is measured against itself with
`dc_geometry::SelfBand` and #221's rules: stretches more than `√2·d`
apart along it, sharp corners and points where it touches or crosses
itself set aside. Where two such stretches come closer than `r` (less
`EPS_MM`), it warns `ToolpathsOverlap` with `shape` and `other` the same
path and `gap_mm` the narrowest gap, before its pairs with later paths.
(changed in #224: a tight spiral or hairpin drawn in the waste cleared
an area with no warning, while the same shape drawn as two lines warned.
It is a warning, not #221's error: the cut is in the waste, as for two
separate paths there (D6, #206). The threshold is D6's `r`, not `d`, so
one line and two lines drawn the same get the same answer. A path that
crosses itself is not warned there, as #221 treats a crossing as where
the line meets itself; two separate crossing paths still warn with gap
0.) (changed in #206: an open
path was dropped before any pair was compared, so a line drawn beside or
across a part, or across another line, cut into it with no warning. A
line inside a part is a cut into that part, not a second shape beside
it, so it follows the same-region rule; whether it needs a band check
like #199's is #210.)

An open path a region holds takes part in the On line band check
instead. `generate_profile` compares it with every line of its region
(the outline and the holes) and with every other open path the same
region holds, and refuses one that comes closer than `d` (less `EPS_MM`)
to another with the same `BandNarrowerThanTool`: the two cuts together
cut through the wall between them, as through #199's ring. A band exactly
`d` wide is cut. Lines that meet are told apart from lines that run
alongside:

- Two lines **meet** where they touch or cross, or come within the
  project's flatten tolerance of each other: a kerf slot drawn from the
  part's edge, a saw cut from edge to edge, two slots joined at a corner,
  a slot from a hole's edge, two slot lines crossing. Their cuts open into
  each other there, as drawn.
- Around each meeting point, the stretch of both lines within `√2·d` of
  it is set aside (`dc_geometry::meeting_points`,
  `distance_away_from_mm`). Two straight lines that meet at an angle θ
  come within `d` of each other for `d / sin θ` from the meeting point,
  so a slot meeting the outline at 45° or more is cut, and one at a
  shallower angle is refused.
- Everywhere else the two lines keep `d`. A slot from the edge that
  then turns to run beside the outline, or runs up beside another edge,
  is refused, with `band_mm` the narrowest gap outside the set-aside
  stretches.

The shapes are named in drawing order. Open paths in different regions,
or held by none, stay D6's. S9 holds an On line Profile's cuts on an open
path to the same rule.

A line is also measured against itself (`dc_geometry::SelfBand`): every
closed line of a region cut On line, and every open path a region holds.
Two stretches of one line closer than `d` cut through the part between
them as two lines do: a hairpin, a spiral with tight turns, an outline
with a neck narrower than the bit (a dumbbell). `generate_profile`
refuses it with `BandNarrowerThanTool`, `shape` and `other` both the
line, `band_mm` the narrowest gap so measured.

- Only points more than `√2·d` apart along the line are compared (the
  shorter way round a closed line), so a line is never compared with the
  stretches next to it. A stretch that turns by at most 90° is at least
  `1/√2` of its length long end to end, so a smooth curve and a corner of
  90° or more never come within `d` of themselves. The line is cut into
  stretches at most `√2·d / 8` long for this, so where along it a
  stretch lies is known to an eighth of that reach.
- **Corners:** a vertex where the line turns by more than 90° is where
  two lines meet, as above: the stretch within `√2·d` of its tip is set
  aside. A V of 45° or more is cut, as a slot meeting the edge at 45° is,
  and a sharper one (a 10° spike, a star whose points are sharper than
  45°) is refused, with the band `√2·d · sin θ`. Where an open path
  touches or crosses itself, or comes within the flatten tolerance of
  itself, it meets itself there too.
- **Which side:** an open path a region holds has the part on both
  sides, so every pair counts. For a closed line only a pair with the
  part between them counts: the point midway lies in the region's
  material, at least a quarter of their gap from the line. A notch
  narrower than the bit cut into an outline, or a hole with a narrow
  neck, has waste between its stretches and is cut; a pair along one edge
  across a notch's mouth has its midpoint on the line and does not count.

An open path drawn twice, the same vertices in either direction, is cut
once: the later copy is dropped with `DuplicateShapeDropped` naming the
first, before any band or overlap check, as `nest` drops a closed shape
drawn twice. S9 drops it the same way.

A part whose outline cut On line lies everywhere within `r` of the line
(its outline offset inward by `r` is empty,
`dc_geometry::inset_is_empty`) is removed whole by the cut, however short
the outline. `generate_profile` refuses it with
`CamError::PartNarrowerThanTool { op, shape, diameter_mm }`, `shape` the
outline, checked before the self-band. Only a region's outline is
checked: a hole's inside is waste, and a hole within `d` of its outline
is refused by the band check first. An island in a hole is a region of
its own, so a tiny one is refused. `inset_is_empty` offsets only an
outline whose area is at most `L·r` (`L` its length), since the inside
within `r` of a closed line covers at most that, so large outlines cost
nothing. S9 refuses every cut on the lines of such a region, measured
with `r` less the program's slack, so it never refuses what the
generator cuts.

(changed in #223: the self-band compares only stretches more than
`√2·d` apart along the line, so an outline shorter than `2·√2·d` (about
18 mm for a 6.35 mm bit) was never compared, and a 2.5 mm radius circle
cut On line vanished with no finding while a 2.9 mm one was refused. A
new variant names the harm, the whole part removed, not a band. It is an
error, consistent with #192, #199, #210 and #221. A tiny dot marked on
purpose is Engrave's job, which has no such rule. A part that is narrow
everywhere and long, such as a 3 mm strip, now gets this error instead
of `BandNarrowerThanTool`.)

(changed in #221: a line was never compared with itself, so a hairpin, a
tight spiral or a dumbbell outline cut On line cut through the part with
no finding, and a duplicate open path in a part was refused with a band
of 0 while a duplicate closed shape was dropped. The rule is #210's
applied to one line: its corners meet as two lines do, so a V turn and
two slots joined at the same angle get the same answer. Only the part
counts for a closed line, because an On line cut already takes `r` from
each side of every line, and clearing waste between two stretches costs
the part nothing more.)

(changed in #210: an open path inside a part, closer than `d` to that
part's own lines, cut through the wall between them with no finding: D6
does not compare a region's own lines, and S9 held the cut to its path
only. The issue asked whether a warning fits better, since an open line
is drawn on purpose more often than a narrow ring. It is an error,
consistent with #192 and #199: the part loses material the user drew as
kept, and when in doubt it is an error. A line drawn beside a wall for
its own sake is Engrave's job, which has no such rule. The issue
proposed setting aside every pair of lines that meet; that would pass a
slot that meets the outline and then runs alongside it, so only the
stretch near the meeting point is set aside. That stretch is `√2·d`, not
a tool radius: a slot square to the edge is within `d` of it for its
first `d`, so a reach of `r` would refuse every slot from the edge.)

### Profile left and right of an open path (dc-cam::profile) (added in #207)

VCarve's 2D Profile cuts an open vector "to the Left, to the Right or On"
it ("Machining Open Vectors", V9.5 reference manual). On came in #175;
`Side::Left` and `Side::Right` add the other two. They are new `Side`
values rather than Inside and Outside reused, so `outside` keeps one
meaning and the labels are VCarve's.

- **Which side.** Left is to the left looking along the path from its
  first vertex to its last, as drawn (VCarve: "Looking along the
  vector(s) from the green node indicates the direction"). Internal
  geometry is Y-up (ADR-0001), so for a segment direction `(dx, dy)` the
  left normal is `(−dy, dx)`. The SVG and DXF importers keep the drawing
  as seen, so left is the left the user sees.
  (`profile::open_path_left_offsets_to_the_left`,
  `profile::open_path_right_offsets_to_the_right`, both with the path
  drawn both ways; `cli::left_profile_of_an_svg_line_exports_on_its_left`
  pins it through the SVG importer.)
- **Offset.** `dc_geometry::offset_open(path, left_mm)`, cavalier_contours'
  open-polyline parallel offset (positive to the left of travel, pinned
  by `offset::open_offset_moves_left_of_travel_and_ends_square`), by `+r`
  for Left and `−r` for Right. An arc keeps its centre and its radius
  grows or shrinks by `r`; a turn away from the tool's side is a round
  join about the vertex, a turn toward it is trimmed where the offset
  edges cross; an arc of radius `r` or less on the tool's side collapses,
  and every stretch closer than `r` to any part of the path is dropped,
  so the result is zero or more pieces, each run in the path's direction.
  A path with none warns `ToolTooLarge`.
- **Ends: square, on the end normals.** The tool centre starts and stops
  on the normal through each end vertex, `r` from it. The cutter's edge
  touches the end vertex and reaches `r` past it along the line on the
  waste side only; nothing on the part side past the end is touched.
  VCarve's documentation does not say how its open offset ends (an open
  question, to confirm against VCarve itself). Square ends are what the
  raw offset gives, keep the cut to the drawn length, and make a part
  drawn as separate lines meet at its outer corners with no gouge, while
  a round extension would cut `r` further along the line. The
  alternatives were rejected: stopping `r` short leaves a nib at the end,
  and wrapping round the end vertex cuts into the part side past it.
- **Closed targets** are skipped with `Warning::SideNeedsOpenPath { shape }`
  (`side_needs_open_path`), not mapped to Inside or Outside by winding:
  the drawing does not show a closed path's direction
  (`profile::left_right_on_closed_target_is_skipped_with_warning`). An
  open target of an Inside or Outside Profile keeps `OpenPathSkipped`,
  whose message now names Left and Right. Validation's `OpenProfile` is
  not reported for a definition an enabled Left or Right Profile targets.
  O8 (`order::targets_of`) reads a Left or Right Profile's open paths
  and none of its closed outlines.
- **Direction** as for On line open paths (#242), on each piece:
  conventional from the end beside the first vertex, climb from the end
  beside the last (VCarve: "Conventional starts cutting at the Start
  point Climb cutting starts cutting at the end point";
  `profile::left_right_direction_follows_vcarve`). VCarve's names pick
  the starting end, not the engagement: with Left, travel from the first
  vertex keeps the part on the tool's right, which is climb by
  `traversal_is_ccw`'s rule; the Profile chapter says so.
- **Entry, tabs, leads, relief** as for On line open paths: a ramp runs
  `entry::along_path` on the offset piece; a helix is
  `HelixNeedsWasteSide` (validation and generator), there being no waste
  domain; tabs are `PathEnds::Open`, measured along each piece from the
  end beside the first vertex as drawn; no lead
  (`LeadDropped { reason: open_path }`, VCarve: "Leads cannot be added to
  open profile toolpaths") and no relief
  (`ReliefSkipped { reason: open_path }`, VCarve: "Sharp External Corner
  options do not apply"); a piece frees nothing
  (`profile::left_right_ramp_and_tabs`). An open path drawn twice the
  same way is cut once (`DuplicateShapeDropped`); drawn the other way it
  is the other side, so it is not a duplicate.
- **Bands.** The side the cutter runs on is waste, the other side is the
  part, so #192's rule applies to the kept side: each tool-centre piece
  keeps `r` (less `EPS_MM`) from every other open path of the operation
  (crossing it would cut what that line keeps) and from every path's
  kept edge, the path offset by `r` to its other side (reaching it would
  cut the strip one radius wide each line keeps beside it). A path is
  held to its own kept edge too, so a spiral or hairpin whose cut side
  faces the part side of another stretch is caught; its own line it keeps
  by construction. Lines that meet are not set aside as #210's are: a
  square end running onto the next line of a part drawn as separate
  lines, cut on its inside, cuts into it. A violation is
  `CamError::CutsIntoKeptSide { op, shape, other, diameter_mm }`, the
  shapes in drawing order and possibly the same path; an error, not a
  warning, as #192, #199 and #210 decided for cuts into kept material
  (`profile::left_right_cut_into_the_kept_side_is_refused`). D6's
  `ToolpathsOverlap` is not run for Left and Right: its condition, a path
  within `r` of another shape's line, is already this error.
- **S9** (`Material::SideProfile`): every cut's ends and middle lie within
  the slack of one of the targets' open paths offset by `r` to the side
  (`offset_open`, computed from the project, not taken from dc-cam), and
  the whole cut keeps `r` less the slack from every open path and every
  kept edge, closed targets ignored
  (`safety::left_right_cut_off_its_path_fails_s9`; the property test
  `random_valid_left_right_profiles_never_panic_and_pass_safety`).

(changed in #207: an open path could be cut only On line, so cutting
beside a line, as VCarve's Left and Right do, needed the line redrawn one
bit radius over. Issue #207's design comment is the source of these
rules; the strip rule was added in implementation, because a cut that
only keeps `r` from other lines still passes when it lies wholly on
another line's kept side, as a line drawn twice in opposite directions
shows.)

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
4. **Pass:** the levels are `generateStepLevels(top, bottom,
   tool.step_down)`; per corner and level: plunge at the descend point,
   cut to the end, cut back. Only corners whose descend point the main
   path has cut at the deepest level are cut (else `ReliefCornerNotCut`);
   links between corners retract. A tool with no step-down skips the pass
   with `ReliefSkipped`.
   - **Pocket:** after the main blocks, corners ordered nearest-neighbour
     from the last exit, one block per level; the blocks share one group.
     Two corners whose descend points are equally near, within
     `NEAREST_TIE_MM`, are taken in drawing order (changed in #215, see
     the sequencer's order step).
   - **Profile:** among each shape's own passes, never after the pass that
     frees it. A corner counts as cut when its own shape's path cuts its
     descend point at the deepest level. Each level goes in the unit of
     the shape's first pass at or below it, which has opened the column at
     the descend point by then, and runs after that pass's lap.
     Consecutive levels of one corner in one unit are one block that steps
     down at the descend point. A shape's corners are ordered
     nearest-neighbour from the end of its first block. The levels of a
     freeing pass are cut from inside its lap instead: where the lap
     (re-seamed by a lead, raised by tabs) first reaches the descend point
     at depth, the tool rises in the column the lap has just opened to the
     first level, cuts out and back at each level, steps down at the
     descend point between them, returns to the lap's Z and carries on. A
     lap move that passes the descend point in its middle is split there;
     at the lap's start the notch runs right after the descent. The rest
     of the lap still holds the part or slug, and nothing of the shape is
     cut after the lap closes. A freeing lap that never reaches the
     descend point at depth (none in practice: the guard needs it) keeps
     the separate block, which the sequencer puts before the lap (Sequence,
     step 2). (changed in #189: a through-cut Profile with no tabs freed
     its part or slug with its last lap, and every notch after that cut
     beside a piece that could shift. Changed in #193: the freeing pass's
     levels ran as blocks before its lap and plunged through the skin, up
     to one step-down, whatever the Profile's entry; cut from the lap,
     every Z change at a descend point is in an open column.)

### Sequence (dc-cam::sequence)

New, per D10 and research §5.5. Pure functions of `SequenceInput`:
`plan` returns the block order (steps 1–3), and `sequence` returns the
toolpath in that order (steps 4–5). A generator that counts contours "in
cut order" in its warnings (`TabsOverlap`, `TabsCoverMostOfContour`,
`ToolpathsOverlap`) plans, warns, then sequences.

1. **Validate:** every block's `cuts` is at `z_mm` (tabs may rise above);
   each shape's passes are consecutive integers from 0; a block with an
   empty descent has `descent_from == entry`. A failure is
   `CamError::InvalidBlock { shape, pass, problem }`; containment pairs
   that leave no shape cuttable are `CamError::ContainmentCycle`.
2. **Units:** a *unit* is one shape's blocks at one pass (full depth
   first groups a shape's units into a *stack*; pass first groups all
   shapes' units of one pass into a *layer*). Blocks within a unit keep
   generator order (`group`), except that the blocks that free material
   come last, so nothing of a shape is cut after its freeing pass. A
   shape whose offset splits into several
   tool-centre loops is one unit per pass: its loops are cut together,
   pass by pass, under both orders (phase 1 cut each loop to full depth
   on its own). (decided in slice 1, #127. Changed in #189: freeing blocks
   last, so a Profile's relief notches at its freeing pass precede the
   lap that frees it. Since #193 those notches are cut from inside the
   freeing lap, so this only orders the fallback block.)
3. **Order:**
   - `FullDepthFirst`: order shapes by a greedy walk: start from the
     first shape in drawing order; from the current shape's exit, pick the
     nearest *available* shape's entry (available = every shape it
     encloses, per `containment`, is finished; ties by drawing order,
     where distances within `NEAREST_TIE_MM` (0.001 mm) of the nearest
     are a tie). Emit all its passes top to bottom.
     (changed in #215: distances were compared exactly, by
     `f64::total_cmp`, so the 1e-6 mm an `f32` SVG import moves a
     coordinate settled a tie, and an SVG and a DXF of one drawing cut two
     equally near shapes or relief corners in different orders. Every
     nearest-first choice now takes the first candidate in drawing order
     whose distance is within `NEAREST_TIE_MM` of the nearest:
     `sequence::nearest_of`, used by the shape walk, Drill's hole order,
     relief corner order, raster piece order, and offset ring order and
     ring start. 0.001 mm is the finest step a post writes in mm, so two
     candidates closer than that cannot be told apart in a program, and it
     is above `f32` noise for coordinates within about 2 m.)
   - `PassFirst`: for pass k, walk the shapes as above but a shape is
     available at pass k only when every enclosed shape has finished pass
     k and, if this is the shape's freeing pass, has finished entirely.
     The walk for each pass starts from the previous pass's last exit.
   - Drill: one pass per hole; the two orders coincide.
4. **Links:** between consecutive blocks `a → b`:
   - same shape, next pass, `b.descent_from` equals the tool's position
     (within `EPS_MM`): no link; `b.descent` starts at `a.z_mm`
     (`descent_from_z_mm`), a straight plunge if empty. Comparing with the
     tool's position, not `b.entry` with `a.exit`, lets a ramp that starts
     at the previous exit stay down; a plunge behaves as before;
   - same shape, next pass, different start (lead-in moved the seam, or
     alternation): rapid up to safe Z, rapid to `b.descent_from`,
     descend;
   - same shape, not the next pass, `b.descent_from` equals the tool's
     position (within `EPS_MM`), `b.descent` opens with a straight plunge
     there (empty, or a first `Linear` move straight up or down at
     `b.descent_from`), and a cut of `a` passed through `b.descent_from`
     (within `CLEARED_POINT_TOLERANCE_MM`) at or below the Z that plunge
     ends at: no link; the plunge runs from where the tool is, straight up
     or down at the plunge rate in the column `a` just cut, then the rest
     of `b.descent`. A Profile's relief notch whose descend point is where
     the lap ended is cut this way. Other shapes keep the retract (O3).
     (changed in #240: the notch lifted to safe Z and came straight back
     down at the same point);
   - `b.cleared` and `a.cleared` are both `Some` at `b.z_mm == a.z_mm`,
     and the link is no longer than the flatten tolerance, or the
     straight link lies inside `a.cleared ∪ b.cleared` and its length
     ≤ `tool_diameter_mm + stepover_mm`: stay down, `Linear` at feed to
     `b.entry`, and `b`'s descent is not cut (the tool is already at
     `b`'s Z in cleared material; replaying a helix would climb back
     through air) (O3). "Inside" means both ends in one piece of the
     union, no crossing of its boundary, the middle of every stretch
     between the points where the link touches the boundary inside, and
     every sample point, at most 0.5 mm apart along the link, inside
     (changed in #232: without the stretches, a raster link under 0.5 mm
     between two ends on the round tool-centre boundary about an island
     corner had no sample point, crossed nothing, and cut 0.024 mm into
     the island);
   - otherwise: rapid up to safe Z, rapid in XY to `b.descent_from`, then
     `b.descent` from safe Z (or a plunge at the plunge rate). When
     `b.descent_from_z_mm` is below safe Z, the tool gets there by a rapid
     straight down that stops `ENTRY_CLEARANCE_MM` above the stock top,
     or that far above the lowest Z a cut already written passed through
     `b.descent_from` (within `CLEARED_POINT_TOLERANCE_MM`, 0.01 mm),
     whichever is lower but not below `descent_from_z_mm`, then a plunge
     at the plunge rate for the rest; then the descent. A tool below a
     descent's start (rule 1, a Pocket's later pass) rises to it at the
     plunge rate. (changed in #214: the plunge ran from safe Z.) A
     straight plunge is reached the same way: an empty descent's plunge
     to `b.entry` at `b.z_mm`, and a descent that opens with a straight
     plunge at `b.descent_from` (a lap that starts on a tab, a Profile's
     corner-relief notch). The rapid stops at the same floor, never below
     the plunge's end, and the plunge covers the rest; the floors count
     the cuts of `SequenceInput::cut_before` too, the main path a
     Pocket's relief pass follows. (changed in #227: a straight plunge
     fed from safe Z through the air; the plaque's 3.175 mm Engrave spent
     3 s of each 3.2 s plunge in air.)
   The first block is reached by a rapid to safe Z then XY; the last is
   followed by a rapid to safe Z. Zero-length moves are never written
   (so a tab over the lap start stays byte-identical to phase 1); dwells
   are kept. (changed in slice 1, #127, and slice 9, #147: rule 1 compares
   `descent_from`, a stay-down link skips the descent, and a retract
   before a below-safe-Z descent plunges first. Changed in #213: a link no
   longer than the flatten tolerance stays down; the last raster line
   ends on the cleared area's chords, the wall ring starts on its arc, and
   the 0.006 mm between them lifted the tool.)
5. **Output:** one `Toolpath`. Every `Rapid` is at safe Z except a
   Drill's descent, one Z rapid down to its `retract_z`, the peck
   rapids inside its cuts, and the rapid straight down from safe Z to a
   ramp, helix or straight plunge (step 4, S2 as changed in #214 and
   #227). The descents
   themselves are `Linear` at feed/plunge, never `Rapid`.
6. **Invariants O1–O10** are the module's tests.

`generate_profile` moves onto `sequence` in the first phase 2 slice
(D10): its per-contour emission becomes blocks; the phase 1 goldens must
stay byte-identical for one-shape cases (O6) and set-identical per level
for the ring case.

### Operation-order warnings (dc-cam::order)

For operations `i < j`, both enabled: `i` frees its targets when it is a
Profile with no tabs and a cutting move of its toolpath reaches Z ≤ 0
(within `EPS_MM`), which means depth ≥ thickness. Any side counts:
Outside and On line free the part, and Inside frees the slug of its
hole. A tabbed Profile frees only the outlines it cuts with no tab: its
holes unless `on_holes`, and holes too short for one ("Tabs per
outline"; a part always keeps one); the tabs hold the rest. (changed in #109: a tabbed Profile
freed nothing, because every outline had tabs.)
Every closed target of `j` nested in one of `i`'s freed outlines, or the
same outline, by `dc_geometry::nest` on the true curves, yields
`CutInsideFreedPart (op: j, earlier_op: i, instance)`. Crossing shapes
do not count, and a shape that encloses the freed outline is never
inside it. Targets are resolved from `Operation::targets()`, so the
check never needs the toolpath of an operation that failed; unresolvable
targets are skipped because validation reports them. Tool hint: the
tools of enabled operations in project order; one `ToolChangeAvoidable
(tool, ops)` per tool whose uses are not one contiguous run, ordered by
first use, with `ops` listing every operation that uses it. A disabled
operation does not split a run. (changed in slice 13, #134: tabs hold
the part, and `nest` is stricter than a centroid test.)

The targets of `j` include the open paths it cuts: every open path of an
Engrave and of an On line Profile (Inside and Outside Profile, Pocket and
Drill skip them; one of no length is skipped). An open path is inside a
freed outline when all of it is inside or on it (every vertex and every
segment's middle, `Region::contains_point` on the outline's region), so
one that crosses the outline does not count, as crossing outlines do not.
An open path frees nothing. (changed in #206: O8 read closed targets
only, so an Engrave or an On line Profile of a line inside a freed part
ran in the loose part with no warning.)

The outlines a tabbed Profile through the stock cuts with tabs are held
only by them (`profile::cut_outlines`, `Hold::Tabs`). Every target of
`j`, closed or open, that lies inside one of them by the same rule
yields, when `j` cuts deep, `CutInsideTabHeldPart (op: j, earlier_op: i,
instance, held)`. `j` cuts deep when its `depth_mm` is more than half the
stock thickness (plus `EPS_MM`), which takes in every cut that reaches
the stock bottom, through-cut allowance and all; with either value unset
it does not (validation reports them). A lighter cut, such as the
plaque's 1 mm Engrave after its tabbed cut-out, is normal practice and
quiet. `held` is
the innermost such outline (the smallest by area: outlines round
one target are nested). It comes in the same order as
`CutInsideFreedPart` and is skipped for a pair `(target, i)` that already
warns `CutInsideFreedPart`, since a loose slug inside a tabbed part is
the worse news. The message says to move `j` before `i`, or, for a hole,
to cut it in `i` with its part ("Tabs per outline"). `CutInsideFreedPart`
is unchanged. (changed in #269: a tab-held part was treated as safely
held, so the two-operation star of `tests/manual/star_with_hole`, part
with tabs first, then its hole, cut the hole in a part held by four
small bridges with no warning; projects migrated from v8 keep such
pairs. Then narrowed in #269 review to deep cuts, so a light cut on a
tab-held part, which is normal practice, does not train users to ignore
the warning.)

### Arc fitting (dc-gcode::arcfit)

Ported from `arcFitting.ts`. Runs in `emit` after the toolpath is in
project space and before the machine transform (the transform is a
translation and signed axis map, never a mirror, so handedness survives;
the fitted centre is transformed like a point).

1. Runs: maximal sequences of consecutive straight cutting moves at one
   Z with one feed, started from a known position. An `Arc` move whose
   sagitta is within `tolerance` joins a run as its chord, and its
   midpoint is checked against the fitted circle like a vertex; deeper
   `Arc` moves break runs (they are already arcs). Fewer than 4 points
   (`MIN_RUN_CHORDS` + 1) → unchanged. (changed in slice 14, #128: a
   Profile of an imported curve puts a shallow corner arc between every
   two facets, for the coaster r 3.175 mm, sweep 2.25°, sagitta
   0.0006 mm, so runs of `Linear` moves alone never pass one move and
   acceptance 37 could not hold.)
2. Greedy longest-first: for `end` from `min(n−1, i+127)` down to `i+3`,
   Kasa least-squares circle through points `i..=end`; project the centre
   onto the perpendicular bisector of the endpoints and set `r` from the
   start (endpoints exact); accept if every point (and every shallow-arc
   midpoint) is within `tolerance` of the circle, every angular step ≤
   90°, the largest step ≤ 4 × the median, the total sweep ≥ 0.5°, and
   the chord ≥ 0.15·r (or a full circle), and at least two chord steps
   are within 4× of the largest. The median gate leaves out the
   shallow-arc joints: their tiny steps are not chord steps, and counting
   them fails every odd-length window on a faceted curve. (changed in
   #213: a 0.006 mm stay-down link, a shallow corner arc and a 40 mm wall
   side lie on a 154.75 mm circle, and with only two chord steps the
   median let it through; the arc bowed 1.3 mm into the wall, which S9
   refused.) And the arc lies within `tolerance` of every move it
   replaces along the move's whole length
   (`arcfit::arc_chord_deviation_mm`: the farthest of the arc's ends, the
   point farthest from the chord's line, and eight points between, from
   the chord; for a chord between points on the circle, its sagitta
   `r(1 − cos(step/2))`); a shallow corner arc counts as its chord.
   (changed in #226, #229, #233: only the vertices were checked, and a
   square's four corners lie on one circle, so a 7 mm square cut On line
   became four 90° arcs bowing 1.45 mm off its sides and failed S9. A
   polyline that stands for a curve has chords within the tolerance when
   it is flattened at a tolerance no coarser than the arc-fit one, so the
   fitter's unit tests fit those, not coarse polygons; a project whose
   `flatten_tolerance_mm` is coarser than `arc_fit_tolerance_mm` fits
   fewer runs.) Direction from
   the sign of the summed cross products (negative = clockwise = `G2`).
3. Split each fitted run into arcs of ≤ 90° with intermediate endpoints on
   the circle and the last endpoint exactly the source point; `I`,`J` per
   sub-arc from its own start.
4. After number formatting (machine coordinates, the post's decimals),
   check each arc as the controller will: with `ij`, snap `I`,`J` within
   ±2 quanta to minimise the radius disagreement; accept when
   `|r_end − r_start| ≤ 0.005 mm`, reject above 0.5 mm, else accept when ≤
   0.1 % of `r`; with `r` format, repair a chord/2 rounding shortfall of
   up to 2 quanta, else reject. Each run is judged from the emitted
   position, with the post's number format; a run with no known emitted
   start falls back (upstream judges it from its own start; this cannot
   happen after a rapid). A rejected sub-arc reverts its whole run to the
   original moves and warns `ArcFitFallback (op, runs)`. A full circle
   never emits as one arc (phase 1 `FullCircleArc`): a run that closes is
   split at 90° so no sub-arc's end equals its start.
5. Not fitted, passed through as `Fitted::Move`: rapids, plunges, dwells,
   descents (helix descents change Z; the flat bottom revolution of a
   helix *is* fitted, which is correct and matches a hand-written
   circle), drill moves, and toolpath arcs deeper than the tolerance
   (lead arcs among them).

**S12 in the safety pass:** `parse_motion` returns each `G2`/`G3`;
`check` takes `Tolerances { flatten_mm, arc_fit_mm }`. Step 3 splits a
run at synthesised points on the circle, so only the run's ends are
toolpath vertices, and S6 checks a chain of sub-arcs, not one arc. It
accepts consecutive `G2`/`G3` where the toolpath has straight moves (and
shallow arcs) when:

- each piece's centre is within `tolerance + one unit in the last place`
  of the previous piece's (this absorbs the per-arc `I`/`J` snap);
- each piece sweeps at most 90°;
- the chain starts and ends on toolpath vertices and covers at least
  three toolpath moves;
- every vertex and shallow-arc midpoint between lies on its piece within
  that tolerance;
- over the part of every piece between a move's ends, the farthest point
  from that move's chord is within that tolerance (changed in #226: an
  arc whose ends and vertices are all on the circle, over a 0.006 mm
  fragment, a short chord and a 40 mm straight side, bowed 1.3 mm off
  the side and S6 accepted it; S12 must not trust the fitter). Each end
  of the move is placed on each piece it spans (changed in #262: a
  vertex within the angle slack, tolerance over radius, past a piece's
  end counts as on that piece, and the next chord's part of the next
  piece was taken from that piece's start; on a helix's flat revolution
  0.17 mm across the slack is a whole 7.5° chord, so S12 measured arc
  the chord does not span, 0.011023 mm off it against 0.011001 allowed,
  and refused the fitter's correct arcs);
- the direction matches the sign of the vertices' turn.

A wrong arc in place of a deep toolpath arc stays `MotionMismatch`. A
failure is `ExportError::ArcFitViolation`. `fitted_arcs_pass_s6`,
`arc_off_the_vertices_fails_s6`, `arc_over_90_degrees_fails_s12`,
`arc_bowing_off_a_long_straight_move_fails_s12` (mm and inch) and
`arc_within_tolerance_of_every_chord_passes`,
`chord_across_a_piece_end_is_measured_on_both_pieces` and
`arcfit::small_helix_flat_revolution_in_inches_exports` (#262) are the
tests. (changed in slice 14, #128: one arc per run could not match a
split run; slice 4, #137, chained each piece's centre to the previous
piece's rather than the first, because snapped centres drift along a
run.)

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
(plunge rate, once). Every line carries its own `Z`, `R` and `Q`/`P`,
because controllers differ in which words they keep from line to line.
Close with `cancel_command` (`G80`). Only the standard codes are written
(`G98`, `G80`, the `Q` word, commands exactly `G81`/`G82`/`G83`/`G73`),
because the safety pass expands only those; `G99` is refused. The
emitter expands its own lines from the rounded numbers with the same
controller model (`canned.rs`) and compares them with the toolpath before
it writes them. Any hole whose cycle is `Expanded`, a missing command, a
non-standard definition field, a missing `reentry_gap_mm` for `G83`/`G73`,
or a difference in motion makes the whole operation expand (one warning
`canned_cycle_expanded (op, reason)`), so a program never mixes cycles and
moves for one operation. The Drill toolpath measures pecks from `R`, as
controllers do, so `G83` and `G73` are written with any retract height.
(changed in slice 15, #148, which wrote them only at `retract_mm: 0`; and
in #153, which moved the toolpath's peck levels to `R`.)

**Chip-break re-entry.** No bundled definition writes `G83` or `G73`, so
peck and chip-break Drills are always plain moves (changed in slice 17,
#164; `definitions/README.md` gives the sources):

- LinuxCNC 2.9 backs both cycles off a fixed 0.254 mm (0.010 in). In
  2.10 (master) the distance is an INI setting, `G83_PECK_CLEARANCE` and
  `G73_PECK_CLEARANCE` (default 1 mm, in machine units), or a `D` word.
  A program cannot know which, so neither command is written.
- Mach3 documents `G83` as "backed off a bit", and its `G73` lift is a
  Pullback setting.
- UCCNC has no canned cycles in its definition: its manual's `G81` text
  has no rapid to `R`, so its motion is not the RS274/NGC motion S11
  models.

`reentry_gap_mm` states both the `G83` re-entry and the `G73` lift; the
definition has no other field for the lift. A definition without it
never writes `G83` or `G73`, and a hand-written one is unparseable.

**S11 expansion** (`safety.rs`): on `G81`–`G83`/`G73` lines, the parser
synthesises the motion the controller performs: rapid XY at the current
Z; rapid to `R`; `G81`: feed to `Z`, rapid to `R` (`G99`) or initial Z
(`G98`); `G82`: plus a dwell; `G83`: feed `Q` increments with a rapid to
`R` and back to the previous depth + the controller's re-entry gap
(documented per controller, written in the README) between; `G73`: feed
`Q` increments with a rapid lift of the re-entry gap between. A cycle is
refused as unparseable when Z is unknown, when neither `G98` nor `G99`
is in force, when a word is missing, or (`G83`, `G73`) when the
definition states no gap. The expansion is then compared with the Drill
toolpath move for move (S6), a Z-only rapid inside a hole matching
within the re-entry gap stated in the definition
(`canned_cycles.reentry_gap_mm`, a new definition field, `null` where no
fixed gap is documented); S1 and S15 still check where that rapid ends.
Only controllers whose documentation states these semantics get
canned-cycle commands in their definition.

### Post-processor definitions (dc-project/src/definitions)

Four new files, `<id>/v1.json`, started from PureCutCNC's and then
checked line by line. `crates/dc-project/src/definitions/README.md` has
one section per controller with the document and section each line was
verified against, a "Not used, and why" list and a "Changed from
PureCutCNC" table; `post::every_definition_line_is_cited_in_readme`
checks that every line is there. The README's sources are the
authority: a source that settles a row the other way makes a new
definition version, not a spec edit. The rows below are what the
definitions write (changed in slice 17, #164: the controllers'
documents disagreed with the spec's expectations in several rows):

| Controller | Sources (README) | As written |
| --- | --- | --- |
| LinuxCNC | LinuxCNC 2.9.10 `g-code.html`, `m-code.html`, `overview.html`, `hal_manualtoolchange(1)`; source on branch `2.9` and `master` | header `( Project: {programName} )`, `G90 G17 G40 G49 G80 G94`, `G91.1`, `G21`/`G20`, `G54`; `M3 S`; `G4 P` seconds; no `%`; no line numbers; no `M6`, no `G43 H`; canned `G81`, `G82` only, with `G98` and `G80`; footer `M5`, `M2`; `.ngc` |
| Mach3 | Using Mach3Mill 1.84-A2; Mach3 Install & Config v3 | `%` (never numbered), no `O0001` (an `O` word is a subroutine label and "The O line may not have a line number", 10.5.2, 10.8.7); line numbers `N10` step 10 on every line but the `%`; `G90 G94 G17 G40 G49 G80`, `G91.1`, `G21`/`G20`, `G54`; `G4 P` **seconds** (the unit is a setting, shown as seconds in both manuals, and `G82 P` is documented in seconds); no `M6`; canned `G81`, `G82` only; footer `M5`, `M30`; `.tap` |
| UCCNC | UCCNC user's manual 1.0047 (software 1.2113) | no `%`, no `O0001`, no line numbers (the manual documents none of them); `G90 G17 G40 G49 G80 G94`, `G54`; no `G20`/`G21` (not in the code list: lengths are in the profile's units, so the program has no units word and `export` warns `units_set_by_controller`); `G4 P` **milliseconds** (the default); no `M6`; no canned cycles; footer `M5`, `M30`; `.txt` |
| generic | RS274/NGC (NIST IR 6556, Kramer, Proctor, Messina 2000) | `( )` comments, no line numbers, `G90`, `G17`, `G94`, `G21`/`G20`, `G54`, `M3 S`, `G4 P` seconds, no `M6`, no canned cycles, footer `M5`, `M30`, trailing zeros off, `.nc` |

Rules for all four (README, "Rules for the slice 17 definitions"):

- **No `M6`, so `"m6"` mode is refused** on every new definition, not
  only on generic. LinuxCNC's `hal_manualtoolchange` documents no
  touch-off at its dialog, and a stock configuration without a changer
  loops `tool-change` back so `M6` does not stop. On Mach3 and UCCNC what
  `M6` does is a user setting, one of whose values is "ignore". Each
  definition's tool change is `M5`, `M0`, and it is never written.
- **No `G43 H`.** Tool length offsets need a tool table DreamCarve does
  not write; the user sets Z zero per tool, as on grblHAL.
- **User text never opens a comment.** LinuxCNC acts on comments that
  start with a keyword (`(LOGOPEN,file)` truncates a file), so the first
  header comment is `( Project: {programName} )`. grblHAL and GRBL got the
  same header in version 2 of each (#171).
- **Header modal codes.** `G40`, `G49`, `G80` and `G91.1` are accepted
  in any program and written again after every pause; `G90.1` is
  refused (#154).

`SpindleDwell` gains `units: Seconds | Milliseconds` so a millisecond
dwell (UCCNC) is written right; the bundled phase 1 definitions carry
`seconds` (see "Migration v7 → v8"). `G4` dwell for Drill uses the same
field. A definition with no units commands writes no units word; S5 then
does not require one, and the parser starts in the program's units. A `%` line is a program delimiter: the parser skips it and the
writer never numbers it. Line numbers wrap after `N99999`. Tool-change
height, `G53` parking and `G43` stay out (#73).

### DXF import (dc-import::dxf)

Parsing by `dc-import`'s own group-code reader, ported from upstream's
`parsePairs`/`extractEntities`/`extractBlocks` (#124 replaced the `dxf`
crate, which pulled in about 50 crates): ASCII and binary DXF (R12
one-byte and R13+ two-byte codes) to (code, value) pairs typed by the DXF
Reference's "Group Code Value Types", then the `$INSUNITS` header
variable, the blocks, and every entity of the ENTITIES section with a
POLYLINE's VERTEX rows attached. A malformed pair is
`ImportError::Dxf(DxfReadError)`. `import_dxf(bytes, options) ->
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
   mirror their bulges negated; a MINSERT (INSERT with 70/71 column and
   row counts, 44/45 spacings) once per cell, the cell offset rotated with
   the insert and not scaled (all expansion together places at most
   `max_vertices / 10` block entities, skipped ones included, else
   `ImportError::TooMuchWork`, so nested arrays of blocks that yield no
   vertices cannot run unbounded); `$MODEL_SPACE`, `$PAPER_SPACE`, names
   starting `_`, and layers `ASHADE`/`HATCH` skipped silently as upstream.
   **Extrusion (210/220/230):** CIRCLE, ARC, LWPOLYLINE, 2D POLYLINE and
   INSERT coordinates are in the entity's OCS (arbitrary axis algorithm):
   normal +Z is the world frame, −Z a mirror in X (bulges negated);
   ELLIPSE keeps its world centre and axis and runs clockwise under −Z;
   any other normal counts as `<TYPE> (not in the XY plane)`. A spline-fit
   POLYLINE (flag 4) drops its frame vertices (VERTEX flag 16); a
   curve-fit one (flag 2) keeps every vertex. Everything else, ATTRIB
   included, counts in `DxfEntitiesSkipped { counts: BTreeMap<String,
   usize> }`, in ASCII and binary files alike.
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
  Adaptive Item (holder). Not an object, or `data` missing →
  `LibraryError::NotAFusionLibrary`; `version` missing →
  `LibraryError::MissingVersion`; below 11 or not an integer →
  `LibraryError::FusionVersion { found }`. These, invalid JSON and a
  damaged archive are the only errors: one bad item never fails the
  whole file.
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

1. **Container:** bytes starting `PK` → zip (`zip` crate, MIT, deflate
   decoding only); the entry named `tools.json` (or the only `.json`) is
   the document, capped at `MAX_DOCUMENT_BYTES` (64 MiB) unpacked against
   zip bombs. An archive that cannot be read, or holds no JSON document,
   is `LibraryError::FusionArchive { message }`. Otherwise the bytes are
   the document.
2. **Per tool, by `type`:** `"flat end mill"`, `"bull nose end mill"`
   (corner radius `RE` ignored; the tool name gets ` (bull nose r<RE>)`),
   `"slot mill"` → `EndMill`; `"ball end mill"` → `BallNose`; `"chamfer
   mill"` → `VBit { included_angle_deg }`; `"drill"`, `"spot drill"`,
   `"center drill"`, `"counter sink"`, `"counter bore"`, `"reamer"` →
   `Drill`; every other milling, hole-making, turning, probe and holder
   type → `Skipped { UnsupportedType(type) }`; a string outside the
   schema's enums → `Skipped { UnknownType(type) }`; an item that is not
   an object or has no `type` → `Skipped { MissingType }`. A chamfer mill
   without `TA` → `Skipped { NoTaperAngle }`.
   - **Chamfer angle.** `TA` is the half angle: `included_angle_deg =
     2·TA`, so a 90° V-bit carries `TA` 45. Settled by the maintainer
     against IDC Woodcraft's published Fusion library (the schema's
     wording, "included angle between generatrixes", reads otherwise, and
     is noted in the module doc so nobody re-derives it).
3. **Units:** every length (`DC`, `stepdown`, `stepover`) and feed
   (`v_f`, `v_f_plunge`) is in `unit` and converted to mm or mm/min; `n`
   is RPM; `NOF` → flutes (0 or absent → 1, noted in the report). A
   `unit` that is missing or not `millimeters`/`inches` →
   `Skipped { UnknownUnit }`; the unit is never guessed.
   `stepover` becomes `stepover / DC` (a fraction, clamped to (0, 1] with
   a report note when above 1); `stepdown` → `step_down_mm`. Both only
   when their `use-*` flag is true or absent.
4. **Presets:** one `Tool` per preset: name `description` for one preset,
   `"<description> (<preset name>)"` for several. A tool with no
   `description` takes its `product-id`, else `<DC> <mm|in> <type>` (for
   example `6.7 mm drill`; Autodesk's metric sample has 179 drills with
   no description). In the bull nose suffix, `RE` is written in the
   file's unit. `source = "fusion:<tool guid>:<preset guid or index>"` so
   a re-import recognises it; a tool with no presets gets
   `fusion:<guid>:no-preset`, and a tool with no guid uses
   `data[<index>]` in place of the guid. No presets → one tool with feeds
   unset. Missing `n`, `v_f` or `v_f_plunge`, or a zero or negative one,
   leave the field `None` (never defaulted; export refuses until set),
   and the report says so. Each `ImportedTool` carries its `notes`
   (`ImportNote`): unset feed, plunge or RPM, no presets, flutes
   defaulted to 1, stepover clamped to 1. A drill preset usually has
   `v_f_plunge` and no `v_f` (Autodesk's sample): the tool imports with
   `feed_mm_min` unset, which Drill never needs but validation reports
   for a tool a Profile uses.
5. `DC ≤ 0` or absent → `Skipped { NoDiameter }`. A mapped tool that
   fails `validate_tool` (for example `TA` ≥ 90) →
   `Skipped { Invalid { field, message } }`.
5a. **Tool number:** `post-process.number` > 0 → `Tool::number`; 0 or
   absent → `None`. Every preset of one Fusion tool shares the number,
   so a library with several presets of one tool holds several tools
   with the same number; `tools add` copies the number, and validation
   reports the clash only when two such tools are used in one project.
6. **Report:** `tools import` prints one line per imported tool
   (`imported`, `imported as <name> (2)`, `replaced` or `already
   present`, then its notes, for example `` `5 mm Drill`: imported (lt6);
   feed unset ``), then one line per skipped item with its reason, then
   the counts. For a Fusion import the counts gain `, N skipped`: `N
   imported, N already present, N replaced, N skipped; library: <file>`.

(changed in slice 21, #152: the skip reasons, errors, notes and name and
source fallbacks beyond the first draft.)

**Tool numbers** (functional open question 7, decided 2026-10-20): the
optional field is added in version 8 (see "Data model", `Tool::number`). Why:
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
crates/dc-geometry/src/{..., boolean.rs (i_overlay union/difference/intersection on Regions), circle.rs (circle test, Kasa fit), self_band.rs (a line against itself, #221)}
crates/dc-project/src/{model.rs (+ PocketOp, DrillOp, EngraveOp, Entry, Lead, CornerRelief, MachiningOrder), migrate.rs (v7_to_v8), validate.rs (+ rules above), post_processor.rs (+ use_canned_cycles, SpindleDwell::units, CannedCycles::reentry_gap_mm), definitions/{README.md, grblhal, grbl, linuxcnc, mach3, uccnc, generic}/v1.json}
crates/dc-cam/src/{lib.rs, toolpath.rs (+ Dwell), block.rs (CutBlock), sequence.rs, order.rs, profile.rs (blocks; entry, lead, relief hooks), pocket.rs, pocket/{rings.rs, raster.rs, rest.rs}, drill.rs, engrave.rs, entry.rs, lead.rs, relief.rs, footprint.rs, passes.rs, tabs.rs, overlap.rs, stock.rs, warning.rs}
crates/dc-gcode/src/{emit.rs (+ canned cycles, split modes), arcfit.rs, canned.rs (controller model of G81–G83/G73), safety.rs (+ S9–S15), parse.rs (+ G2/G3 fitted, G8x expansion, G4, `%`), post.rs (definition checks)}
crates/dc-import/src/{svg.rs, dxf.rs, dxf/{groups.rs, document.rs, entities.rs, spline.rs, stitch.rs}}
crates/dc-library/src/{lib.rs, library.rs, fusion.rs, error.rs}
crates/dreamcarve/src/{main.rs, cli.rs, init.rs, export.rs, check.rs, tools.rs, config.rs}
tests/golden/{pocket-offset-rect, pocket-offset-island, pocket-raster-0deg, pocket-raster-45deg, pocket-rough-then-rest, drill-simple, drill-peck, engrave-open-closed, profile-ramp, profile-helix-inside, profile-lead, profile-dogbone, order-full-depth-first, order-pass-first, post-linuxcnc-coaster, post-mach3-coaster, post-uccnc-coaster, post-generic-coaster}/
tests/fixtures/{plaque.svg, plaque.dxf, dxf/*.dxf, fusion/{sample.tools, sample.json}}
crates/dc-project/tests/fixtures/dcv/{v7, v8}/sample.dcv
```

New dependencies: `i_overlay` (MIT, booleans; already a settled decision),
`zip` (MIT, `default-features = false`, `features =
["deflate-flate2-zlib-rs"]`: deflate decoding only, the Fusion `.tools`
container), `toml` (MIT/Apache-2.0, the CLI only). `NOTICE` entries as
their licenses require (`zip` and `nix` are listed). `directories` is not
used: it pulls in `option-ext` (MPL-2.0), and std finds the config
directory (see "User config"). (changed in slice 18, #126, and slice 21,
#152.)

## Test plan

Names are `crate::module::test`. "(port)" marks a PureCutCNC test to port;
"(golden)" a corpus comparison. Every emitted-motion test runs
`assert_safe`.

**dc-geometry**
- `boolean::difference_of_rect_and_circle_has_one_hole`, `boolean::union_of_touching_rects_is_one_region`, `boolean::intersection_of_disjoint_is_empty`.
- `circle::sixteen_facets_are_a_circle_within_tolerance`, `circle::two_bulge_arcs_are_a_circle`, `circle::ellipse_is_not_a_circle`, `circle::kasa_fit_recovers_centre_and_radius`.
- `self_band::hairpins_are_narrow_and_square_corners_are_not`, `self_band::a_neck_in_a_closed_outline_is_narrow`, `self_band::sharp_corners_meet_and_set_their_arms_aside`, `self_band::a_crossing_is_a_meeting`, `self_band::far_from_leaves_out_the_stretches_near_the_cut`, `distance::closest_points_follow_arcs_and_ends` (#221).

**dc-project**
- `migrate::v7_profile_gains_phase_2_fields_with_defaults` — fixture loads with `machining_order: full_depth_first`, `entry: plunge`, `lead: null`, `corner_relief: none`, `arc_fit_tolerance_mm: 0.01`, `use_canned_cycles: false`; `migrate::v7_to_v8_writes_the_defaults`, `migrate::v7_with_phase_2_field_already_present_fails`.
- `cli::v7_project_exports_the_same_program` (the spec's `v6_fixture_exports_phase_1_bytes_with_fitting_off` at version 8).
- `serialize::tool_number_round_trips`, `validate::duplicate_tool_number_is_error`, `validate::set_number_clashing_with_positional_is_error`, `validate::tool_number_zero_is_error`, `emit::set_tool_number_is_written_as_t_word_and_file_name`, `emit::unset_tool_number_falls_back_to_position`.
- `serialize::pocket_drill_engrave_round_trip`, `serialize::entry_variants_serialize_as_string_or_object`, `serialize::clearance_strategy_raster_carries_angle`.
- `validate::rest_of_must_be_an_earlier_enabled_pocket_with_the_same_targets`, `validate::step_over_fraction_outside_unit_interval_is_error`, `validate::pocket_without_step_down_is_error`, `validate::peck_without_depth_or_tool_step_down_is_error`, `validate::helix_on_on_line_profile_is_error`, `validate::lead_radius_must_be_positive`, `validate::ramp_angle_range`, `validate::canned_cycles_on_definition_without_them_is_error`, `validate::drill_with_v_bit_warns`, `validate::relief_without_step_down_warns`.

**dc-cam — sequence (O1–O10)**
- `sequence::hole_is_cut_before_its_outline`, `sequence::island_is_cut_before_the_hole_around_it`, `sequence::four_levels_cut_innermost_first` (O1).
- `sequence::orders_cut_the_same_moves` (O2).
- `sequence::links_between_shapes_retract_to_safe_z`, `sequence::stay_down_link_only_inside_cleared_area` (O3).
- `sequence::full_depth_first_z_transitions` (port: `testPocketFeatureFirstOrder`, `[-2,-4,-2,-4]`), `sequence::pass_first_z_transitions` (port: `testPocketLevelFirstOrder`, `[-2,-4]`) (O4).
- `sequence::equidistant_shapes_keep_drawing_order`, `sequence::nearest_neighbour_orders_a_c_b_as_a_b_c` (port: `testPocketFeatureFirstNearestBlockOrder`), `sequence::same_input_twice_is_identical` (O5), `sequence::equidistant_shapes_within_tolerance_keep_drawing_order`, `relief::equidistant_corners_follow_drawing_order_despite_noise` (#215).
- `sequence::single_shape_is_identical_in_both_orders` (O6).
- `sequence::pass_first_waits_for_enclosed_shapes_freeing_pass`.
- `sequence::nothing_is_cut_after_a_shapes_freeing_pass` (#189: freeing blocks last in their unit; O1 still holds).
- `profile::one_shape_golden_is_byte_identical_after_sequence` (phase 1 goldens unchanged).
- `tabs::final_pass_has_exactly_four_raised_segments_at_tab_z` extended to both orders (O9).

**dc-cam — tabs per outline (#109)**
- `tabs::holes_get_no_tabs_by_default`, `tabs::on_holes_opt_in_keeps_hole_tabs`, `tabs::too_short_outline_gets_fewer_tabs_and_warns`, `tabs::short_part_outline_keeps_at_least_one_tab`.
- `profile::star_and_its_hole_cut_in_one_operation` (the manual `tests/manual/star_with_hole` case, redrawn).
- `order::tabbed_profile_frees_the_outlines_it_leaves_untabbed`.
- `order::cut_inside_tab_held_part_warns`, `order::deep_cut_inside_tab_held_part_warns`, `order::shallow_engrave_inside_tab_held_part_is_quiet`, `order::inside_first_is_quiet`, `order::one_operation_part_and_hole_is_quiet`, `order::tab_held_warning_names_the_innermost_outline`, `cli::check_warns_hole_cut_in_tab_held_star`, `cli::plaque_engrave_after_tabbed_cut_out_is_quiet` (#269).
- `migrate::version_8_fixture_loads`, `migrate::v8_tabs_keep_their_holes_tabbed`, `migrate::v8_with_on_holes_already_present_fails`.

**dc-cam — pocket**
- `pocket::offset_rings_are_concentric_and_spaced_by_stepover` — 100×50, 6.35 mm, 40 %: ring gaps 2.54 ± 0.01.
- `pocket::offset_cuts_inner_rings_before_the_wall` (port: "offset cuts inner loops before wall-adjacent loops").
- `pocket::wall_pass_first_last_none` — ring order and the undersize with `none`.
- `pocket::footprint_stays_inside_region` (S9), `pocket::footprint_stays_outside_island` (S9).
- `pocket::offset_coverage_reaches_inset_region` — union of footprints ⊇ inset by r within 0.05 at step-over 0.4, 0.6, 0.8 and 1.0 (port: `pocketClearance.test.ts` "no stock left" spirit).
- `pocket::offset_above_half_step_over_leaves_no_centre_pin`, `pocket::raster_above_half_step_over_leaves_no_ridge`, `pocket::rings::leaf_gets_a_residual_ring_only_when_needed`, `pocket::rings::square_corners_get_residual_rings_at_full_step_over` (#258).
- `pocket::corner_cleanup_rings_do_not_lift` (#267: 1 lift per pass at step-over 1.0 with the wall pass last, 2 with it first, as at 0.8; `pocket::rings::leaf_gets_a_residual_ring_only_when_needed` and `pocket::rings::square_corners_get_residual_rings_at_full_step_over` check the splices).
- `pocket::island_wall_runs_opposite_to_pocket_wall` (port: `pocketFinishIslandDirection.test.ts`).
- `pocket::climb_on_pocket_wall_is_counter_clockwise`, `pocket::climb_on_island_is_clockwise`, `pocket::conventional_reverses_both` (Y-up re-derivation).
- `pocket::tool_wider_than_region_warns_and_skips`.
- `pocket::island_splits_region_into_two_tool_centre_regions`.
- `pocket::pocket_inside_island_is_cut_after_island`.
- `pocket::raster_lines_at_zero_degrees_are_horizontal_and_spaced`, `pocket::raster_lines_at_45_degrees`, `pocket::raster_lines_alternate_direction`, `pocket::raster_link_stays_down_inside_region`, `pocket::raster_retracts_across_island`, `pocket::raster_segments_split_around_island`, `pocket::raster_ignores_direction_wall_pass_honours_it`.
- `pocket::raster::scan_starts_half_a_step_below_the_top`, `pocket::raster::closing_line_when_the_scan_stops_short` (#195).
- `pocket::rings::ring_starts_at_nearest_point_of_an_arc`, `pocket::ring_starts_at_nearest_point_not_nearest_vertex` (#196).
- `pocket::raster_lifts_around_island_are_minimised`, `pocket::raster::pieces_round_a_square_island_are_cut_nearest_first`, `pocket::raster::scan_order_is_kept_round_a_circle` (#150).
- `pocket::raster_links_follow_a_round_island_edge`, `pocket::edge_link_never_enters_the_island`, `pocket::raster::pieces_round_a_circle_link_along_its_edge`, `safety::raster_edge_links_round_an_island_pass_s9` (#202; `pocket::raster_lifts_around_island_are_minimised` updated).
- `pocket::raster_links_follow_a_concave_wall`, `pocket::edge_link_never_leaves_the_wall`, `safety::raster_edge_links_along_a_concave_wall_pass_s9` (#218).
- `pocket::allowance_leaves_material_on_walls` — footprint stays `allowance` from the wall.
- `pocket::rest_region_is_corners_and_wall_band`, `pocket::rest_coverage_with_rough_reaches_small_tool_inset` (acceptance 22), `pocket::rest_of_that_clears_nothing_warns`, `pocket::rest_footprint_stays_inside_region` (S14).
- `pocket::step_down_from_tool_when_operation_unset`, `pocket::depth_below_bottom_is_error`.
- `pocket::levels_rapid_at_safe_z_before_plunging` (port: "level transitions rapid at safe Z").
- `pocket::helix_in_narrow_slot_falls_back_to_ramp` (7 mm slot), `pocket::helix_in_small_hole_falls_back_to_plunge` (7 mm hole) (acceptance 27).

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
- `relief::profile_notches_precede_the_freeing_pass` (#189: inside and outside, both orders; the bottom level within the freeing lap since #193), `cli::pocket_dogbone_exports_and_passes_safety` (an inside Profile through the stock: a full lap at the bottom, closing after the last notch).
- `relief::bottom_level_notches_do_not_plunge` (#193: ramp entry, both orders, one and two levels in the freeing pass, with a lead: no straight plunge below what is already cut at a descend point; every bottom notch within the final lap).

**dc-cam — profile**
- `profile::inside_cut_in_band_narrower_than_tool_is_refused`, `profile::inside_band_as_wide_as_the_tool_is_cut`, `profile::inside_island_in_an_outline_too_small_for_the_tool_is_refused` (#192), `profile::on_line_ring_narrower_than_tool_is_refused`, `profile::on_line_ring_wider_than_the_tool_is_cut` (#199), `profile::open_path_alongside_its_part_wall_is_refused`, `profile::open_slot_from_the_edge_is_cut`, `profile::open_path_far_from_walls_is_cut` (#210), `profile::on_line_hairpin_narrower_than_tool_is_refused`, `profile::on_line_spiral_with_tight_turns_is_refused`, `profile::on_line_smooth_curve_is_not_compared_with_itself`, `profile::on_line_narrow_notch_in_an_outline_is_cut`, `profile::duplicate_open_paths_are_dropped_with_a_warning` (#221), `profile::on_line_part_smaller_than_the_bit_is_refused`, `profile::on_line_part_just_larger_than_the_bit_is_cut`, `safety::on_line_part_smaller_than_the_bit_fails_s9` (#223).
- `profile::start_under_a_tab_moves_so_the_ramp_fits`, `profile::tab_positions_do_not_move_when_the_seam_moves` (#176).
- `profile::open_path_left_offsets_to_the_left`, `profile::open_path_right_offsets_to_the_right`, `profile::left_right_on_closed_target_is_skipped_with_warning`, `profile::left_right_direction_follows_vcarve`, `profile::left_right_ramp_and_tabs`, `profile::left_right_cut_into_the_kept_side_is_refused`, `profile::left_right_tool_too_large_for_a_tight_arc_warns`, `offset::open_offset_moves_left_of_travel_and_ends_square`, `offset::open_offset_rounds_outer_turns_and_trims_inner_ones`, `offset::open_offset_of_a_tight_arc_toward_its_centre_vanishes`, `safety::left_right_cut_off_its_path_fails_s9`, `cli::left_profile_of_an_svg_line_exports_on_its_left` (#207).
- `profile::on_line_cuts_an_open_path`, `profile::open_path_direction_picks_the_end_it_starts_at`, `profile::on_line_open_path_ramps_back_and_forth_along_the_line` (acceptance 26), `profile::open_path_tabs_are_measured_from_its_first_vertex`, `profile::on_line_open_path_with_no_length_is_skipped`, `profile::open_path_is_skipped_with_warning` (Inside and Outside), `validate::open_profile_cut_on_line_is_not_reported`, `safety::on_line_profile_of_an_open_path_passes_s9` (#175).

**dc-cam — overlap**
- `overlap::open_on_line_path_too_close_to_a_part_warns`, `overlap::open_line_far_from_shapes_is_quiet`, `overlap::open_lines_too_close_to_each_other_warn`, `overlap::open_path_in_a_part_is_not_compared_with_its_lines`, `engrave::crossing_lines_do_not_warn_toolpaths_overlap` (#206), `overlap::self_close_open_path_in_the_waste_warns`, `overlap::open_curve_in_the_waste_does_not_warn_against_itself` (#224).

**dc-cam — order**
- `order::cut_inside_freed_part_warns` (O8), `order::inside_before_outside_is_quiet` (O8), `order::inside_profile_through_frees_its_slug`, `order::operations_keep_project_order`, `order::non_adjacent_same_tool_hints`, `order::adjacent_same_tool_is_quiet`.
- `order::open_path_inside_freed_part_warns` (#206: an Engrave and an On line Profile of a line in a freed part warn; an Inside Profile of it is quiet), `order::open_path_crossing_a_freed_outline_is_quiet` (#206).

**dc-gcode — arcfit**
- Ports from `arcFitting.test.ts`: `accepted_circular_run_gives_four_arcs`, `clockwise_is_g2`, `rejects_straight_run`, `rejects_z_changing_run`, `rejects_mixed_feed_run`, `rejects_discontinuous_run`, `splits_around_rapids`, `short_run_stays_linear`, `rejects_ambiguous_direction`, `rejects_high_residual_ellipse`, `splits_large_arc_at_90_degrees`, `split_arc_offsets_relative`, `rejects_near_collinear_run`, `rejects_long_straight_with_corner_fragment` (inch tolerance), `accepts_ordinary_arc_after_collinear_gate`, `full_circle_loop_is_four_arcs`, `partial_arc_with_straight_leads`, `adjacent_runs_share_endpoints_exactly`, `fitted_arcs_pass_through_source_endpoints`, `ij_derived_from_previous_emitted_point` (#447), `invalid_run_falls_back_to_original_moves`, `fallback_leaves_valid_runs_alone`, `r_format_rejects_unrepresentable_chord`, `inch_output_uses_millimetre_budget`, `snapping_never_worsens_agreement`, `r_format_repairs_rounding_shortfall_only`.
- `arcfit::coaster_circle_exports_as_at_most_eight_arcs_per_lap`, `arcfit::null_tolerance_reproduces_phase_1_program`, `arcfit::helix_descent_is_not_fitted_flat_revolution_is`.
- `safety::fitted_arcs_pass_s6`, `safety::arc_off_the_vertices_fails_s6`, `safety::arc_over_90_degrees_fails_s12` (S12).

**dc-gcode — emit, canned cycles, posts**
- `emit::drill_simple_writes_g81_per_hole_after_g98_and_ends_g80` (port: `testCannedSimpleG81`), `emit::dwell_writes_g82_with_p` (port), `emit::peck_writes_g83_with_q` (port), `emit::chip_break_writes_g73_with_q` (port), `emit::grbl_never_writes_canned_cycles` (port: `testRegressionGrblNoCannedCycles`), `emit::canned_cycles_off_by_default`, `emit::mixed_cycles_expand_whole_operation_with_warning`, `emit::expanded_dwell_writes_g4`, `emit::uccnc_dwell_is_milliseconds` (UCCNC's dwell is milliseconds, Mach3's seconds; slice 17, #164).
- `safety::canned_cycle_expansion_matches_drill_toolpath` (S11), `safety::canned_cycle_below_floor_fails_s1`, `safety::canned_cycle_rapid_into_uncut_fails`.
- `safety::ramp_steeper_than_plunge_fails_s10`, `safety::helix_feed_scaled_passes_s10`.
- `safety::pocket_cut_outside_region_fails_s9` (on the toolpath via `footprint`).
- `safety::profile_ramp_into_part_fails_s9`, `safety::profile_lead_arc_into_neighbouring_part_fails_s9`, `safety::inside_profile_cut_outside_hole_fails_s9`, `safety::lap_beside_a_close_part_passes_s9_and_a_cut_off_it_fails`, `safety::outside_notch_passes_s9_only_with_relief`, `safety::generated_profiles_pass_s9`, `safety::s9_check_is_fast_on_twenty_profile_parts` (#177), `safety::inside_profile_lap_in_a_band_narrower_than_the_tool_fails_s9` (#192), `safety::on_line_profile_in_a_ring_narrower_than_the_tool_fails_s9` (#199), `safety::on_line_open_path_cutting_through_a_wall_fails_s9` (#210), `safety::on_line_hairpin_fails_s9`, `safety::s9_check_is_fast_on_an_on_line_outline_of_many_chords` (#221).
- `post::six_definitions_validate`, `post::linuxcnc_mach3_uccnc_generic_differ_only_in_dialect` (acceptance 36), `post::mach3_numbers_every_line`, `post::uccnc_writes_no_line_numbers_percent_or_units_word`, `post::generic_strips_trailing_zeros`, `post::generic_refuses_m6_mode`, `post::linuxcnc_mach3_uccnc_refuse_m6_mode`, `post::canned_cycles_only_where_documented`, `post::no_definition_has_g43`, `post::every_definition_line_is_cited_in_readme` (a test that scans the README for each non-placeholder header/footer/tool-change line).
- `emit::one_program_flag_writes_m6_on_grblhal`, `emit::one_program_flag_on_grbl_is_error`, `emit::programs_per_tool_flag_overrides_m6_mode_without_saving`.

**dc-import — dxf**
- `dxf::insunits_mm_inch_cm_m_scale_correctly`, `dxf::missing_insunits_is_error_without_override`, `dxf::units_override_wins`.
- `dxf::line_arc_circle_lwpolyline_import`, `dxf::arc_keeps_centre_and_radius_as_bulge`, `dxf::circle_is_two_bulge_arcs`, `dxf::bulge_sign_is_not_flipped` (no Y flip), `dxf::polyline_with_vertex_entities`, `dxf::closed_flag_closes_polyline`, `dxf::spline_flattens_within_tolerance`, `dxf::ellipse_flattens_within_tolerance`.
- `dxf::insert_applies_translation_rotation_scale`, `dxf::nested_insert_accumulates`, `dxf::mirrored_insert_negates_bulges`, `dxf::non_uniform_scaled_arc_is_flattened`, `dxf::insert_depth_limit_warns`, `dxf::system_blocks_and_layers_skipped`.
- `dxf::paper_space_entities_skipped`, `dxf::unsupported_entities_counted_in_one_warning`.
- `dxf::layer_names_become_shape_names`, `dxf::layer_filter_keeps_only_named_layers`, `dxf::block_entities_on_layer_0_inherit_insert_layer`.
- `dxf::four_lines_stitch_into_closed_shape`, `dxf::gap_beyond_tolerance_stays_open`, `dxf::stitching_does_not_cross_layers`, `dxf::identical_shapes_are_deduped`.
- `dxf::plaque_dxf_matches_plaque_svg_within_0_01` (acceptance 34), `dxf::coordinate_limit_applies`.
- `dxf::negative_extrusion_mirrors_x`, `dxf::tilted_extrusion_is_skipped_with_warning`, `dxf::minsert_places_every_cell`, `dxf::huge_minsert_of_empty_block_fails_fast`, `dxf::nested_minserts_fail_fast`, `dxf::spline_fit_polyline_drops_frame_vertices`, `dxf::curve_fit_polyline_keeps_every_vertex` (#123).
- `dxf::swallowed_types_counted_in_binary_dxf`, `dxf::binary_dxf_imports_like_ascii`, `dxf::parser_rejects_odd_line_count_and_bad_codes`, `dxf::insert_attributes_are_counted`, `dxf::non_utf8_names_are_read_as_windows_1252` (#124).
- `dxf::cp1251_layer_names_decode`, `dxf::unknown_code_page_warns_and_falls_back`, `dxf::utf8_file_is_unchanged` (#209).

**dc-library**
- `library::round_trip_is_byte_identical`, `library::add_detects_already_present`, `library::find_by_id_then_unique_name`, `library::ambiguous_name_is_error`, `library::validation_rules_from_upstream` (port of `parseToolLibraryFile` rules: positive diameter, flutes ≥ 1, stepover in (0, 1], V-bit angle in (0, 180)).
- `fusion::zip_and_plain_json_both_read`, `fusion::flat_end_mill_maps_with_geometry_and_preset`, `fusion::inch_tool_converts_to_mm`, `fusion::chamfer_mill_is_v_bit_with_double_taper`, `fusion::drill_types_map_to_drill`, `fusion::two_presets_become_two_tools`, `fusion::no_presets_imports_with_feeds_unset`, `fusion::missing_feed_stays_none`, `fusion::stepover_length_becomes_fraction`, `fusion::use_stepdown_false_leaves_step_down_unset`, `fusion::unsupported_types_skipped_by_name`, `fusion::unknown_keys_are_ignored`, `fusion::not_a_library_is_error`, `fusion::sample_library_imports_every_supported_type_and_skips_the_rest` (the hand-written fixture: flat, ball, bull nose and slot mills, chamfer mill, drill, spot drill, counter sink, counter bore, reamer import; thread mill, tap, face, form, lollipop, dovetail and tapered mills, a turning tool, a probe and a holder skipped by name), `fusion::chamfer_mill_ta_45_is_a_90_degree_v_bit`, `fusion::drill_preset_without_v_f_imports_with_feed_unset`, `fusion::tool_number_is_read_when_non_zero`, `fusion::tool_number_zero_is_none`, `fusion::schema_version_below_11_is_rejected`, `fusion::missing_description_names_tool_by_diameter_and_type` (slice 21, #152).

**dreamcarve (integration, `tests/`)**
- `cli::plaque_svg_exports_two_programs_in_order` (acceptance 18), `cli::plaque_dxf_exports_same_programs_as_svg`.
- `cli::init_dxf_without_units_asks_for_flag`, `cli::init_dxf_layer_and_join_flags`.
- `cli::tools_import_fusion_reports_each_tool`, `cli::tools_import_twice_is_already_present`, `cli::tools_add_copies_into_project_with_new_id`, `cli::library_edit_does_not_change_project`, `cli::tools_export_import_round_trip`, `cli::config_path_prints_locations`, `cli::config_dir_env_override`.
- `cli::config_tool_change_mode_seeds_init`, `cli::one_program_and_programs_per_tool_flags` (acceptance 33).
- `cli::check_reports_order_warnings`, `cli::warnings_print_code_and_message`.
- `cli::posts_lists_six`.
- `cli::broken_pipe_does_not_panic`, `cli::stdout_write_error_fails_with_message` (Linux only; #216).
- `cli::canned_cycles_linuxcnc_opt_in` (acceptance 24).

**dc-gcode (integration, `tests/golden.rs`)**
- `golden::pocket_offset_rect`, `golden::pocket_offset_island`, `golden::pocket_raster_0deg`, `golden::pocket_raster_45deg` (`.camj` angle −45; README explains), `golden::pocket_rough_then_rest` (PureCutCNC rough + finish with `stockToLeaveRadial`; compared as per-level sets, union coverage asserted), `golden::drill_simple`, `golden::drill_peck`, `golden::engrave_open_closed`, `golden::profile_ramp` (edge route with `entryStrategy: ramp`; the along-the-path ramp differs from upstream's zigzag, so this case compares the lap moves only and asserts the ramp separately; README says so), `golden::profile_helix_inside`, `golden::profile_lead`, `golden::profile_dogbone`, `golden::order_full_depth_first` and `golden::order_pass_first` (three parts with holes; **compared in order**, not as sets), `golden::post_{linuxcnc,mach3,uccnc,generic}_coaster` (motion equal to phase 1's coaster; dialect lines compared to the verified definitions, not to PureCutCNC's).

## Slices

Ordered; each is one issue and one PR. Dependencies in brackets.

1. **Sequence module and Profile on blocks.** `block.rs`, `sequence.rs`,
   `generate_profile` emitting blocks; O1–O10 tests; phase 1 goldens
   unchanged. Tests: dc-cam sequence group, `profile::one_shape_golden…`.
2. **Format v8: model, migration, validation** (v7 in the first draft; #106 took it). All new types with
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
