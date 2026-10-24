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
| `dc-cam::bore` | `src/engine/toolpaths/drilling.ts` (the `helical` drill type: `HELICAL_BORE_MAX_HOLE_TO_TOOL_RATIO`, its too-small and too-large checks), `src/engine/toolpaths/entry.ts` (`emitCenterLockedCircularBore` in finish-bore mode) | Its own operation, not a Drill cycle (maintainer decision 2026-10-24). Pitch per turn (Fusion's "Pitch"), not a ramp angle. Starts 1 mm above the stock top (#214), not at a retract height reached through the hole's centre. Chords per turn from the flatten tolerance, at least 48. The bottom revolution is four exact arcs, not 48 chords, and is followed by a move to the centre (Fusion's "Lead To Center") before the lift. Radial allowance added. The too-small limit is a 0.05 mm helix radius, not `hole ≤ tool`. Helix sense from `entry::helix_is_ccw` (Y-up), not `helixAngularDirection`. |
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

### dc-project (format version 11)

```rust
pub const CURRENT_VERSION: u32 = 11;  // #106 took 7 (slice 2, #117); #109 took 9; #207 took 10; #105 took 11

pub struct Project {
    // phase 1 fields unchanged, plus:
    /// Fit arcs to straight runs at export within this deviation. `None`
    /// turns fitting off. `init` writes 0.01.
    pub arc_fit_tolerance_mm: Option<f64>,
    /// The project's placed tabs (version 11, #105, ADR-0005), saved after
    /// `operations`. `init`: empty; migration: empty.
    pub tabs: Vec<PlacedTab>,
}

/// A tab placed on one outline of one instance (#105). Every enabled
/// Profile that cuts that outline rises over it ("Placed tabs").
pub struct PlacedTab {
    pub id: TabId,               // "tab<n>"; unique in the project
    pub instance: InstanceId,    // the instance whose outline holds it
    pub profile: usize,          // index into the instance's definition's profiles
    pub x_mm: f64,               // the tab's point in definition space (Y-up):
    pub y_mm: f64,               //   on the outline, or on the tool path it was placed for
    pub width_mm: f64,           // along the outline; > 0
    pub height_mm: f64,          // tab top above the stock bottom; > 0
    pub shape: TabShape,         // "rect" | "smooth"
    pub holds_slug: Option<bool>, // #302, format 15: meant to hold its hole's slug;
                                 //   null from the v14 → v15 migration
}

pub struct Tool {
    // phase 1 fields unchanged; `step_over_fraction` now read by Pocket.
    /// Who makes the tool and the maker's part number (format 16,
    /// maintainer decision 2026-10-24), saved right after `name`. Set by
    /// Fusion import from `vendor` and `product-id`; nothing in a program
    /// reads them. Migration: null.
    pub vendor: Option<String>,
    pub product_id: Option<String>,
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

pub enum Operation { Profile(ProfileOp), Pocket(PocketOp), Drill(DrillOp), Bore(BoreOp), Engrave(EngraveOp) }  // Bore: format 18

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

/// From version 11 a *tab request*: tabs to place on every outline the
/// Profile cuts ("Placed tabs"). Generation places it before cutting;
/// `dreamcarve tabs` commands place it on save and set it to `None`.
pub struct TabSpec {
    // phase 1 fields unchanged, plus (version 9, #109):
    /// Holes get tabs too: the outlines nested as holes among the
    /// operation's targets. `init`: false; migration: true.
    pub on_holes: bool,
}

pub struct ProfileOp {
    // phase 1 fields unchanged (`tabs: Option<TabSpec>` is now the
    // request; `init` still writes one: 4 rect tabs, 8 × 3 mm), plus:
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
    pub allow_tool_wider_than_hole: bool,     // format 17: drill a hole narrower than the tool
                                              // at the tool's size; false skips it (changed:
                                              // maintainer decision 2026-10-24)
}

/// Format 18 (maintainer decision 2026-10-24): helical bore of each
/// circular target to its drawn size, as Fusion's "2D Bore". `"kind":
/// "bore"`, fields saved in this order.
pub struct BoreOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>, pub tool: ToolId,
    pub depth_mm: Option<f64>,                // from stock top; None until set
    pub pitch_mm: Option<f64>,                // Z per helix turn; None: tool.step_down_mm; (0, step-down]
    pub direction: CutDirection,              // init climb (counter-clockwise)
    pub allowance_mm: f64,                    // radial stock to leave; init 0; ≥ 0
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

**Migration v10 → v11** (`migrate::v10_to_v11`, #105, ADR-0005): add
`"tabs": []` right after `operations`, and change nothing else. A
Profile's `tabs` stays as a tab request: placing it needs the Profile's
tool paths, which only `dc-cam` builds, and a migration step rewrites
JSON only. Generation places a request with the rules `tabs place`
uses, so an upgraded file cuts byte-identically with no step by the
user; the next `dreamcarve tabs` command that saves the file places the
requests of its enabled Profiles ("Placed tabs"). The migration fails if
`tabs` is already there. The fixtures
`crates/dc-project/tests/fixtures/dcv/{v10, v11}/` load as the sample
project (`migrate::version_10_fixture_loads`,
`migrate::v10_project_gains_an_empty_tab_list`,
`migrate::v10_with_tabs_already_present_fails`). Byte identity is
checked end to end on a corpus: the projects of
`crates/dreamcarve/tests/fixtures/tabs-migration/` (13 made for it, plus
`plaque.dcv`, `plaque-svg.dcv` and a version 6 grblHAL file), every
golden case with tabs, and the coaster `init` writes each export the
same G-code as before #105, both as loaded and after `tabs place` has
saved their tabs as placed tabs (`cli::version_9_tab_settings_export_byte_identical_programs`,
the golden harness). One change is intended: once placed, two Profiles
that cut one outline both rise over the tabs of both. (changed in #105:
tabs were an operation setting.)

**Migration v15 → v16** (`migrate::v15_to_v16`): add `vendor: null` and
`product_id: null` right after each tool's `name`, and change nothing
else; the migration fails if either key is already there. A tool
without them is a tool as it was, and no program reads them, so every
file exports the same G-code. The fixture
`crates/dc-project/tests/fixtures/dcv/v15/sample.dcv` loads as the sample
project (`migrate::version_15_fixture_loads`,
`migrate::v15_tools_gain_no_vendor`). (changed: maintainer decision
2026-10-24. A tool had no vendor or product id. If another change takes
16 first, this is the next free version, rebased on top.)

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
(`Problem::EngraveDeeperThanCone`). Placed tabs (#105), as errors: two
tabs with one id (`Problem::DuplicateTabId`), a tab on an instance that
does not exist (`TabDanglingInstance`) or on a profile its definition
does not have (`TabProfileMissing`), a tab point beyond the coordinate
limit, and a `width_mm` or `height_mm` that is not more than 0.

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
pub fn generate_bore(project: &Project, op: &BoreOp) -> Result<(Toolpath, Vec<Warning>), CamError>;   // format 18
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

pub mod placed_tabs {   // #105, "Placed tabs"
    pub struct Placement { pub tabs: Vec<PlacedTab>, pub warnings: Vec<Warning> }
    pub struct TabSize { pub width_mm: f64, pub height_mm: f64, pub shape: TabShape }
    /// A request's layout on the tool paths of Profile `op`, as placed tabs.
    pub fn place(project: &Project, op: &OperationId, spec: &TabSpec) -> Result<Placement, CamError>;
    /// A tab at each distance along every outline `op` cuts (`tabs place --at`).
    pub fn place_on_outlines(project: &Project, op: &OperationId, at_mm: &[f64], size: TabSize, on_holes: bool) -> Result<Placement, CamError>;
    /// Every enabled Profile's request placed and cleared, as `dreamcarve` saves.
    pub fn place_requests(project: &Project) -> Result<(Project, Vec<Warning>), CamError>;
    /// The tabs `op` crosses replaced by what `place` places without them (#293).
    pub struct Replacement { pub project: Project, pub removed: Vec<PlacedTab>, pub placement: Placement }
    pub fn replace<F: FnOnce(&Project) -> Result<Placement, CamError>>(project: &Project, op: &OperationId, place: F) -> Result<Replacement, CamError>;
    pub fn crossed_by(project: &Project, op: &ProfileOp) -> Result<Vec<TabId>, CamError>;
    pub fn check(project: &Project, generated: &[(&Operation, &Toolpath)]) -> Vec<Warning>;  // tab_not_crossed, tab_off_outline
    /// The tab's width plus `diameter_mm` (#300).
    pub fn off_outline_limit_mm(tab: &PlacedTab, diameter_mm: f64) -> f64;
    /// How far the tab's point is from its outline, project space (#300).
    pub fn off_outline_mm(project: &Project, tab: &PlacedTab) -> Option<f64>;
    /// The placed tabs `op` crosses on a path round a hole, with the hole (#300).
    pub fn on_holes(project: &Project, op: &ProfileOp) -> Result<Vec<(TabId, ShapeRef)>, CamError>;
    pub fn tab_point(project: &Project, tab: &PlacedTab) -> Option<Point>;   // project space
}
// CamError::UnknownOperation { op }, CamError::TabsNeedProfile { op, kind },
// CamError::InvalidTab { op, field, value_mm }                    (#105)
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
`DrillTargetNotCircle (op, instance, profile)`, `DrillHoleTooSmall (op,
instance, hole_mm, tool_mm)` (the hole is skipped), `DrillToolWiderThanHole
(op, instance, hole_mm, tool_mm)` (drilled anyway at the tool's size,
under `allow_tool_wider_than_hole`), `DrillNoHoles (op)` (every target
skipped, so the operation drills nothing; changed: maintainer decision
2026-10-24), `DrillPeckDeeperThanHole (op, peck_mm,
depth_mm)` (once per operation: every hole has the same depth),
`DrillRetractBelowStockTop (op, requested_mm, used_mm)` (a negative
`retract_mm` is raised to the stock top), `BoreTargetNotCircle (op,
instance, profile)`, `BoreNoRoom (op, instance, hole_mm, tool_mm,
allowance_mm)` (the helix radius is under `BORE_MIN_HELIX_RADIUS_MM`; the
hole is skipped), `BoreLeavesCore (op, instance, hole_mm, tool_mm,
allowance_mm, core_mm)` (the hole is skipped), `BoreNoHoles (op)` (format
18; maintainer decision 2026-10-24), `EngravePathDegenerate (op,
instance, profile)`, `CutInsideFreedPart (op, earlier_op, instance)`,
`CutInsideTabHeldPart (op, earlier_op, instance, held)` (#269),
`ToolChangeAvoidable (tool, ops)`, `RestOfClearsNothing (op, rest_of,
tool)`, `TabOnHole (op, tab, instance, instance_name, profile)` and
`TabNotCrossed (tab, instance, instance_name, profile)` (#105; the second
names no operation), and `TabOffOutline (tab, instance, instance_name,
profile, off_mm, limit_mm)` (#300; names no operation). All phase 1 codes stay. (changed in slices 4, 7 and 9, #137,
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
A Bore's stays within `ρ − r − allowance` of the centre of one of its
holes (radius `ρ`, Drill's circle test), the same hole for a move's ends
and middle (changed: maintainer decision 2026-10-24).
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
An Inside or Outside Profile's ramp or helix below the stock top, in a
run of falling moves that started below it, runs only on the bit's path
round its targets or along an earlier cut of the operation (#352). An
earlier cut counts only where it ran below the stock top itself, no
slack taken: the part of each cut below the top is recorded, not the
whole cut. (changed in #356: a whole cut was recorded once any of it ran
below the top less the slack, which includes the arc-fit tolerance, so a
ramp leg that dipped 0.015 mm below the top at its end counted as trench
along its whole length with arc fitting off, and not at all with it on.)
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
pub const LIBRARY_VERSION: u32 = 2;   // 2: tools carry `vendor` and `product_id`
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

**Library migration v1 → v2** (`library::load`): the library file stores
`Tool` as a `.dcv` does, so it gains `vendor` and `product_id` too. A
version 1 file is rewritten in memory, before strict deserializing, with
`vendor: null` and `product_id: null` after each `tools[].tool.name`
(failing if either is there already), so an old library loads with both
`None`; the next write saves version 2. A version 1 file passed to `tools
import` migrates the same way. Tests: `library::version_1_file_migrates`,
`library::round_trip_is_byte_identical` (with the fields set and unset).
(changed: maintainer decision 2026-10-24. The library was version 1.)

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
10. **Corner relief** (`relief` module): with `cleared = region inset by
    allowance` and `walls = tool-centre region's root rings`, collect
    corners, then splice each corner's levels into the wall or island
    ring of each pass that reaches its descend point, before the blocks
    are sequenced ("Corner relief", step 5). Corners with no such ring,
    and every corner of a Pocket with an allowance, get notch blocks in
    their own group at the end, stepping down at the tool's step-down
    (step 6). (changed: maintainer decision 2026-10-24: every notch was
    in that group at the end.)
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
   diameter `d = 2 × mean radius`. A hole the tool does not fit, `tool >
   d + flatten_tolerance_mm`, is skipped with `DrillHoleTooSmall` naming
   the hole, `d` and the tool diameter, unless the operation's
   `allow_tool_wider_than_hole` is set, when it is drilled and warns
   `DrillToolWiderThanHole`. The tolerance is the one the circle test
   accepted the outline at: the drawing gives the hole's size no closer
   than that, so a hole drawn at the bit's size (a 0.25 in DXF circle and
   a 6.35 mm bit, a circle flattened to facets) is drilled, and one
   visibly smaller (6.3 mm at the default 0.01 mm) is not. If no hole is
   left, every target having been skipped for either reason, the
   operation warns `DrillNoHoles` once and its toolpath is empty: export
   still writes the other operations, and the warning says why this one
   cuts nothing. (changed: maintainer decision 2026-10-24; a hole the tool
   was wider than was drilled with `DrillToolWiderThanHole`, compared
   within `EPS_MM`.)
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

### Bore (dc-cam::bore)

(added: maintainer decision 2026-10-24; functional capability 14a.) The
algorithm is PureCutCNC's `helical` drill type ("PureCutCNC sources"),
with the changes listed there. Fusion's "2D Bore" names the settings
(Autodesk Fusion help, "Bore reference": Pitch, Direction, radial Stock
to Leave, Lead To Center; "Generate a Bore toolpath"). Fusion documents no
default pitch, so DreamCarve takes the tool's step-down, as Drill's peck
does.

1. **Resolve holes.** Every closed profile of every target, placed by its
   transform, through Drill's circle test (`dc_geometry::as_circle` at
   the flatten tolerance); anything else warns `BoreTargetNotCircle` and
   is skipped. With hole radius `ρ`, tool radius `r` and allowance `a`,
   the helix radius is `R = ρ − r − a`.
   - `R < BORE_MIN_HELIX_RADIUS_MM` (0.05 mm): skipped with `BoreNoRoom`.
     At 48 chords a turn a helix that small has chords 2·0.05·sin(3.75°) =
     0.0065 mm long, more than √2 units of the coarsest place a
     post-processor writes in inches (0.0001 in), so every chord's ends
     round apart (the rule `entry` applies to its helix, #265); below it
     the bit is within 0.1 mm of the hole, which is a Drill's job, and
     the message says so when `2ρ ≥ tool − flatten_tolerance_mm`.
   - `R − r > flatten_tolerance_mm`: skipped with `BoreLeavesCore`, `core_mm
     = 2(R − r)`. The bit's swept ring is `[R − r, R + r]` about the centre,
     so beyond `2ρ = 2·d + 2a` it leaves a core. **Refused, not warned or
     cleared:** a through bore frees the core at its last turn, and a loose
     core spins against the bit and can be thrown; a blind one leaves a
     pin, which is not the hole drawn. Clearing the core with more helices
     or rings is a Pocket, which already clears a circle with tested
     coverage and a helix entry, so Bore stays one helix and one circle
     and the message points to Pocket. PureCutCNC refuses the same
     holes (`HELICAL_BORE_MAX_HOLE_TO_TOOL_RATIO = 2`). Within the
     tolerance the bit's edge passes the centre and leaves at most a pin
     of that radius, no larger than the drawing's own error.
   - No hole left: `BoreNoHoles` once, and the toolpath is empty.
2. **Heights and settings.** `top = thickness`, `bottom` the last of
   `passes::pass_z_mm(top, depth, depth, through_cut_allowance)` (S1),
   `safe = top + safe_z_mm`, `start = entry::descent_start_z_mm(top,
   safe)` (1 mm above the top, #214). Pitch `p = pitch_mm.or(tool
   step-down)`, else `CamError::MissingStepDown`. `p` finite and more than
   0, at most the tool's step-down (plus `EPS_MM`) when it has one, and
   `a` finite and at least 0, else `CamError::InvalidBoreSetting { op,
   field, value }`. The tool's feed and plunge rate are required
   (`MissingFeed`). Validation reports the same problems first.
3. **Helix**, per hole about centre `c`. Sense `entry::helix_is_ccw(direction,
   CutSide::Internal)`: climb is counter-clockwise. Chords per turn `n =
   max(48, ⌈π / acos(1 − tol/R)⌉)` so a chord bows in at most the flatten
   tolerance `tol`, but no more than `⌊π / asin(√2·quantum / 2R)⌋`, so a
   chord still runs √2 units of the last place in XY (#265's rule; added
   in #387: a tiny tolerance on a large helix asked for chords too short
   to write). Turns `T = (start − bottom) / p`; chords `N = ⌈T·n⌉`,
   at least 1. `N > entry::MAX_DESCENT_MOVES`, or a chord that falls less
   than one unit of the program's last place (`(start − bottom)/N <
   quantum`, the S3 rule `entry`'s helix keeps, #265), is
   `InvalidBoreSetting` on `pitch_mm`. Point `k` is at angle `θ_k = sense ·
   2π·T·k/N` from +X, radius `R`, `z_k = start − (start − bottom)·k/N`:
   Z is linear in the angle, so each turn falls exactly `p` and the last
   less. The helix starts at `c + (R, 0)` at `start`. Each chord's feed is
   `entry::plunge_limited_feed_mm_min` (S10).
4. **Finish.** From the helix end, at `bottom`: one full circle of radius
   `R` as four 90° arcs in the helix's sense (`ArcCcw` for climb) at the
   cutting feed, back to the helix end; then a `Linear` to `c` at the
   feed. No core is left (step 1), so the centre is cut and the move is in
   air; the bit leaves the wall before the lift, as Fusion's "Lead To
   Center", rather than dragging up it.
5. **Blocks.** One per hole: `pass` 0, `z_mm = bottom`, `descent_from` the
   helix start at `descent_from_z_mm = start`, `descent` the helix chords,
   `entry` the helix end, `cuts` the circle and the move to `c`, `exit =
   c`, `frees: false` (nothing comes loose), `cleared: None`. Sequenced as
   Drill's (`full_depth_first`, no containment, links at safe Z, at the
   clearance Z between instances, a rapid to safe Z at the end): the
   sequencer rapids straight down to `start` (S2 as changed in #214).
6. Off-stock check as every generator (#62).

Arc output: the helix is straight `G1` chords, each changing Z, as an
entry helix's are; the bottom circle is four planar arcs. No helical
`G2`/`G3` is written. The toolpath's arcs are planar (`MoveKind`), and a
helical arc would need S6, S9, S12, the footprint, the estimate and the
`rs274`, GRBL and LinuxCNC simulators to follow Z along an arc; the chords
cut the same wall within the flatten tolerance, and the circle at the
bottom is exact. Arc fitting leaves the descending chords as lines
("Arc fitting"), and every bundled definition writes the planar arcs as
`G2`/`G3` (`arc_interpolation: true`, IJ or R format) or as chords where
the definition or the controller's radius check says so, as it writes a
lead arc. So no definition changes.

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
  if that pass cut none. A ramp after the first pass keeps to the line
  the previous pass ramped on, the only stock beside the kerf that pass
  cut: below the tab top, after a pass that ramped on the plain plan,
  it ramps along that line from the shared target as far as the domain
  less the tab keep-outs holds it, stopped `safety` short
  (`EntryPlan::following`); with no run left, a target the two plans do
  not share, or a pass above that plunged, helixed or cut another path
  (a last pass after its allowance passes), it plunges one step down at
  its target with `EntryFallback`, `no_room`; and one that would fall
  less than `RAMP_MIN_LAST_DROP_MM`, whose one run runs the whole line,
  plunges with `too_small`. The tab re-seam below judges a candidate the
  same way. (changed in #352: below the tab top the tabbed plan's own
  chord was cut, a line no earlier pass had cleared, so the first such
  pass ramped up to `3·D` into full-height stock at the feed.) Either
  ramp runs only as far from the target as the previous pass's ramp ran
  at least one unit in the last written place below the stock top
  (`CutEntry::reach_mm`; one unit, so the program's rounding of each Z by
  half a unit keeps that stretch below the top): when its zigzag would
  run farther, it zigzags on its line cut short there
  (`EntryPlan::cut_short`), and with no run left it plunges with
  `no_room`. (changed in #356: a first pass's ramp starts at the entry
  clearance above the stock, so its first runs cut nothing, yet a later
  pass's ramp ran its whole line; on a shallow ramp it ran 0.69 mm past
  the first pass's trench, about twice the step-down into uncut stock.)
  A contour cut without a lead whose passes below
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
  refuses). The ramp then returns along the path at pass Z to the start
  vertex, and a level piece of that return running less than √2 units in
  XY is merged the same way; one that cannot be is kept (#321: a leg
  ending 0.00015 mm past a chord vertex went back to it first, a move
  the program writes as nothing, which arc fitting counted as one of a
  fitted arc's three chords and S12 did not). The pass then cuts the
  full path from the start vertex, so
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

A tab request (`TabSpec`) gets its own layout on each tool-centre path,
`Tabs::fit`. (changed in #105: tabs were an operation setting, and placed
tabs waited for the GUI. These rules now lay out a request, whether
generation places it before cutting or `tabs place --count` writes it as
placed tabs; placed tabs are cut as placed, "Placed tabs" below.)

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
   (`profile::cut_outlines`, `Hold::Free`). A tab is a request's or a
   placed tab the Profile crosses. (changed in #105: placed tabs count.)

PureCutCNC sizes auto-tabs per feature (`autoTabs.ts`, a 15 % free-path
rule) as a command that writes placed tabs; with #105 that port can write
`Project::tabs` and needs no format change. VCarve applies tabs per
vector and lets the user leave vectors out, which `on_holes`
approximates and placed tabs now do outright. (changed in #109: one tab
setting applied to every outline, holes included, and a too-short
outline kept tabs covering most of it, warned but cut as set; a star and
its 7 mm hole took two operations.)

### Placed tabs (dc-cam::placed_tabs)

A tab is a project object (#105, ADR-0005), not an operation setting.
The ADR records the decisions; this section is the algorithm.

**Representation.** `PlacedTab` names an instance, one profile of its
definition, and a point (`x_mm`, `y_mm`) in definition space, with its
own `width_mm`, `height_mm` and `shape`. In definition space, the tab
follows its instance through every transform (move, rotate, mirror,
scale). It is a point, not a distance along the outline: an Outside
path turns round each convex corner on an arc of radius `r`, and every
point of that arc is nearest the corner itself, so a distance along the
outline cannot say where on the arc a request's tab was; in the
tabs-migration corpus 3 of 13 projects had such a tab. A point keeps
every position a tool path can have. A tab placed on the outline
(`--at`, later the GUI) is the outline's point; a tab placed from a
request is the request's centre on that Profile's tool path, `r` off the
outline for Inside and Outside.

**Forward mapping.** For each enabled Profile and each of its tool paths
of the tab's outline (same instance and profile), the tab goes on the
path nearest it, centred at the path's point nearest the tab's point in
project space (`dc_geometry::nearest_point`, which reports the point,
its distance along the path from the first vertex, and the gap). Ties go
to the earlier segment, except that an arc centred on the point (within
`EPS_MM`) gives its middle, and the segment ends beside it tie with that
middle: a tab placed at a convex corner of the outline comes back at the
middle of the path's corner arc, not at one end of it. The tab's foot is
the outline's point nearest the tab (`placed_tabs::outline_foot`; a tab
on the outline is its own foot).

**One tab alone.** A single tab whose span (width plus diameter) covers
its whole path overlaps nothing: it warns `tabs_cover_most_of_contour`
only, not `tabs_overlap`, for a request and a placed tab alike (changed
in #105 review: it warned both, and as a placed tab got the words for
the one tab a part keeps).

**Crossing.** A Profile crosses a tab when that nearest path point lies
within `r + w/2` of the foot (`w` the tab width): closer, the cutter
would cut into the tab. The cut must also go below the tab's top: with
the stock thickness and the Profile's depth set, a tab whose top is at
or below the cut's floor (`thickness − depth`) is not crossed, so a
shallow score pass of a tabbed outline neither rises nor warns
`tabs_outside_cut_range` (changed in #105 review: depth was ignored).
With either unset every tab in reach counts. A path farther off, such as an Outside path past
a narrow notch the tool cannot reach into, does not cross it.
`crossed_by` lists the tabs a Profile crosses; only the tab's own
outline counts, so a Pocket or an Engrave over the spot never rises,
and another operation's path does not either. Within one Profile a lap
of another outline still rises over the tab where its bit reaches it
("Reached tabs" below). (changed in #362: no other outline's path rose.)

**Spans.** Each crossed tab gives a span of `w + d` along the path
centred at its path point, with its own top and shape, the phase 1 rect
lift and the phase 2 smooth bridge (ramps of `w/2`, held for `d`, the
plunge-rate feed limit of #157). A pass rises over a tab only when it is
below that tab's top (`tabs.rs`, per-tab spans). Spans that touch or
overlap merge as before: the merged span takes the highest top and is
smooth only if every tab in it is, and the path warns `TabsOverlap`.
A request's tabs and the placed tabs on one path are laid together.

**Reached tabs** (#362). A lap rises over every tab its bit reaches, not
only its own outline's. Where a band between two outlines of one
Profile is narrower than the bit (a hole round an island, two parts
whose kerfs overlap: the gouge `ToolpathsOverlap` warns of), one
outline's lap runs within the radius of the other's tab and would cut it
away below its top. The tab is the material the other outline's last
pass leaves standing, which is found as an allowance path's tabs are
from its own last pass (A8 step 3, `last_pass::twin_tabs`): a point
within `r` of the last pass stands as high as the lowest the last pass
cuts within `r` of it. Each part path, and each allowance path, is
raised over every stretch whose cutter reaches that material of another
part path, at the tab's top, rect if the tab is and smooth with slopes
no longer than keep it above the material if not, a lattice step longer
at both ends (`profile::reached`). These stretches merge with the path's
own spans (`PathTabs::merged`): the merged span takes the highest top
and is rect if any part of it is. They do not count as tabs of that path
(no `TabsOverlap` or `TabsCoverMostOfContour`, and an untabbed hole
stays `Hold::Free`). A path whose bit reaches no other outline's tab is
laid as before. The new stretches' ends are come down from as every
span's are: each pass below the tab top comes down one step-down into
what the pass before cut there, and a last pass comes down a tab taller
than the step-down in stages (A8 step 4, #348). S9's tab rule is
unchanged and holds the result. (changed in #362: each lap rose over its
own outline's tabs only, and a hole's lap in a band narrower than the
bit came down inside an island's tab, which S9 refused.)

**Cut as placed.** Placed tabs are never reduced. When the spans on a
path cover more than half of it, `TabsCoverMostOfContour` warns with the
"All of them are cut, as placed" wording, and every tab is cut; only a
request is reduced (#109's rules). A placed tab on an outline the
Profile cuts as a hole (`Role::Hole`) is cut too, because the user
placed it, and warns `TabOnHole (op, tab, shape)`: the slug is usually
waste and holding it is work by hand. Generation does not warn for a tab
that holds the slug: one whose `holds_slug` is `true`. `place` with
`on_holes` (a request, the migrated `on_holes: true` request of a version
8 file among them, or `--count --on-holes`) and `place_on_outlines` with
`on_holes` (`--at --on-holes`) write `true` on each tab they put on a
hole, since the user asked for it, and `false` on every other tab they
place. A tab whose `holds_slug` is `None`, as the version 15 migration
leaves every format 14 tab, keeps #300's rule: it holds the slug when its
point lies on this Profile's tool path round the hole (within `EPS_MM`),
where only `place` with `on_holes` put a tab. `placed_tabs::on_holes`
lists every hole tab a Profile crosses, and `tabs place` and `tabs clear`
warn `TabOnHole` once for each tab they place on a hole. A hole tab
marked `false` (a hand edit) warns on every export, wherever it sits; one
marked `true` warns on none, on the hole's outline or after a change of
bit or side. The flag changes no cut, and a last pass's allowance path
carries no warning of its own. (changed in #300: every hole tab warned on
every `check` and `export`, and a user who kept the tabs a migrated
request asked for could not silence it. Changed again in #302: #300 told
a tab placed for the cut by its point alone, so a tab placed with `--at
--on-holes`, or left off the path by a change of bit or side, warned on
every export.)

**Migration v14 → v15** (`migrate::v14_to_v15`, #302): `holds_slug:
null` at the end of every placed tab, and nothing else. Whether a format
14 hole tab was placed for the cut shows only in where it sits against
that Profile's tool path, which only `dc-cam` builds, and a migration
step rewrites JSON only; `null` keeps #300's rule for it, so every format
14 file warns and cuts as it did. The migration fails if a tab already
has the key. Byte identity and the warnings are checked on a corpus of
format 14 files with placed tabs, saved by the build before #302:
`crates/dreamcarve/tests/fixtures/holds-slug-migration/`, the 16 tabbed
projects of the #285 corpus with their requests placed and three squares
with a hole (`--at --on-holes`, tabs placed for the cut then the bit
changed, and `--count --on-holes` off corners with a last pass and a start
point); its programs and the warnings of `export` and `check` are
committed under `tests/expected/holds-slug-migration/`, beside the #284
corpus's programs.

**Not crossed.** `placed_tabs::check(project, generated)` gives one
`TabNotCrossed (tab, shape)` for each tab no enabled, generated Profile
crosses: its outline is cut by no enabled Profile (only by a Pocket, by
a disabled Profile, or not at all), or every path runs too far from it.
It is project-level, so it names no operation. `check` and `export` run
it after `order::check` and print its warnings with the others.

**Off its outline** (#300). The same `check` gives `TabOffOutline (tab,
shape, off_mm, limit_mm)` for a crossed tab whose point is more than
`off_outline_limit_mm` from its foot: the tab's width plus the widest
tool diameter among the generated Profiles that cross it. `off_mm`
(`off_outline_mm`) is measured in project space, as cut. A tab placed on
the outline is 0 off and one placed from a request or `--count` `r`, so
neither warns; a point a whole tab and bit away was not written by a
placing command. The tab is still cut at its foot. A tab no Profile
crosses warns `TabNotCrossed` only.

**Placing from a request** (`place`). The request is laid on the
Profile's tool paths exactly as #109 lays it ("Tabs per outline",
`count` from `L/(2n)`, `at_distances` modulo `L`, holes none unless
`on_holes`, the coverage reduction with at least one on a part), on the
path before it is re-seamed for a lead or an entry (#176), so the
positions are those the request cut. Each centre `q` on the path, in project space,
is taken to definition space by the inverse of the instance's transform
(`Transform::inverse`; a singular transform places nothing). The round
trip is exact: the forward mapping's nearest path point of `q` is `q`
itself, so a placed request cuts byte-identically to the request,
including a tab on a corner arc. An open path's centre is measured from
its first vertex as drawn, whichever end the cut starts at.

**Placing on the outline** (`place_on_outlines`, `tabs place --at`). One
tab at each distance along each outline the Profile cuts, measured in
definition space from the outline's first vertex as drawn; a closed
outline takes the distance modulo its length, an open one clamps it to
its ends. Holes are skipped unless `on_holes`, and a tab put on one is
`holds_slug: true` (#302). The point stored is the
outline's point there. These tabs are cut as placed; placing them warns
`TabsOverlap` and `TabsCoverMostOfContour` as generation would.

**Ids and copies.** New tabs take `tab<n>` from one more than the
largest `tab<n>` in the project, so a removed id is not reused. A tab
equal to one the project already has, or to one placed in the same
call (same instance, profile and shape, point, width and height within
`EPS_MM`), is not added, so placing the same tabs twice adds none.

**Replacing** (`replace`, `tabs place --replace`, #293). The tabs a
Profile crosses (`crossed_by`, the set `tabs clear <dcv> <op>` removes)
are removed, then `place` or `place_on_outlines` runs on the project
without them, so a new tab at a removed tab's point is placed again
rather than taken for a copy. The new tabs are renumbered from one more
than the largest `tab<n>` the project had before the removal, so no
removed id names a new tab. (changed in #293: placing only added.)

**Requests.** Generation places any request still present before it
cuts (`profile::plan_tabs`), so a file with requests, of any version,
cuts as before with no step by the user. Every `dreamcarve tabs`
command that saves the project first runs `place_requests`: the request
of every enabled Profile is placed and set to `None`, so the saved file
holds placed tabs only. A disabled Profile keeps its request, which
would otherwise be placed for a cut that does not run; `tabs place
<dcv> <op>` with neither `--count` nor `--at` places it by name. `tabs
list` places requests in memory only and says so. `export`, `check`,
`export --post` and `tools add` leave requests as they are. `init`
writes its Profile's request, as phase 2 did, and an empty `tabs`: its
one Profile targets every closed shape, so tabs placed at init would be
left on shapes the user later cuts with other operations.

(changed in #105: tabs were an operation setting. Changing a Profile's
tool or side moved every tab, and two Profiles that cut one outline each
rose over their own tabs only; once placed, both rise over the tabs of
both. PureCutCNC's tabs are project-level objects too, but each is a
rectangle in the world that any path crossing it lifts over; here a tab
names its outline.)

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
  (changed in #342: the offset of the end segment ends square, but where
  that segment is too short to have one, on the inside of the turn
  before it (shorter than `r·cot(θ/2)` for segments meeting at θ), the
  offset of the segment before runs on until cavalier_contours' circle
  of radius `r` round the end vertex stops it, past the normal: a 30° V
  with a 2 mm last segment, cut inside with the 6.35 mm bit, took about
  0.05 mm² of the kept strip there at full depth, and more of the part
  past the end. `profile::check_open_ends` refuses a path with a piece
  that ends on that circle and none that ends square there:
  `CamError::EndSegmentHasNoRoom { op, shape, vertex, length_mm,
  diameter_mm, allowance_mm }`, at the end vertex as drawn. Clipping the
  piece back to the normal instead, as the issue first proposed, would
  stop it more than `r` from the vertex with nothing cut beside the end
  segment, where it crosses the normal at all; the bit cannot follow
  that end, so it is an error. Where the end segment has room, every
  piece ends square already, so programs that do not reach past an end
  are unchanged. Round the V (the turn's outside) the path is cut. The
  test is `side_last_pass::open_path_short_end_segment_keeps_the_kept_strip`,
  a G-code heightfield of 30°, 45° and 60° V notches with last segments
  of 1, 2 and 3 mm, Left and Right, with and without a last pass and
  tabs at the ends, and `profile::left_right_end_segment_with_no_room_is_refused`.)
- **Closed targets** are skipped with `Warning::SideNeedsOpenPath { shape }`
  (`side_needs_open_path`), not mapped to Inside or Outside by winding:
  the drawing does not show a closed path's direction
  (`profile::left_right_on_closed_target_is_skipped_with_warning`). An
  open target of an Inside or Outside Profile keeps `OpenPathSkipped`,
  whose message now names Left and Right. Validation's `OpenProfile` is
  not reported for a definition an enabled Left or Right Profile targets.
  O8 (`order::targets_of`) reads a Left or Right Profile's open paths
  and none of its closed outlines.
- **Direction** is the milling mode, as for closed shapes, on each
  piece (`profile::open_runs_reversed`). The spindle turns clockwise from
  above, so climb keeps the part on the right of travel
  (`traversal_is_ccw`'s rule). The tool runs on the waste side and the
  part lies beyond the path, so:

  | Side | Climb starts beside | Conventional starts beside |
  | --- | --- | --- |
  | Left | the first vertex | the last vertex |
  | Right | the last vertex | the first vertex |
  | On line (#242) | the last vertex | the first vertex |

  With Left, travel from the first vertex keeps the part on the tool's
  right, which is climb; Right is the mirror
  (`profile::left_climb_keeps_the_part_on_the_right_of_travel`,
  `profile::right_climb_keeps_the_part_on_the_right_of_travel`,
  `profile::left_conventional_is_the_reverse`, each with the line drawn
  both ways). VCarve's start-point rule ("Conventional starts cutting at
  the Start point Climb cutting starts cutting at the end point") is kept
  for On line only, where both flanks cut and no milling mode exists.
  (changed in #287: #207 applied VCarve's start-point rule on every side,
  so a Left cut named `climb` milled conventional and the other way
  round; on a Left or Right cut only one flank meets kept material, so
  climb and conventional are physical and must mean what they mean on a
  closed shape, and in VCarve's and Fusion's milling-mode options.)
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
  within `r` of another shape's line, is already this error. (changed in
  #342: the kept edge alone leaves the strip open at each end, so a cut
  could reach into it past an end normal while keeping `r` from the line
  and the edge. The strip is closed at each end by
  `dc_geometry::strip_ends`: the normal through the end vertex, `r` long
  to the kept side, cut short at the first line of the operation it
  meets, so that where the end segment is short and turns toward the
  kept side the end stops at the segment it runs into. Each piece keeps
  `r` from those too, `r + a` for the allowance paths:
  `profile::left_right_cut_into_the_strip_past_an_end_is_refused`.)
- **S9** (`Material::SideProfile`): every cut's ends and middle lie within
  the slack of one of the targets' open paths offset by `r` to the side
  (`offset_open`, computed from the project, not taken from dc-cam), and
  the whole cut keeps `r` less the slack from every open path, every
  kept edge and every strip end (`strip_ends`, #342), closed targets
  ignored (`safety::left_right_cut_off_its_path_fails_s9`; the property
  test `random_valid_left_right_profiles_never_panic_and_pass_safety`).
  The strip ends make the kept-strip test independent of how the offset
  ends: before #342 a cut along the offset of a path whose offset ran
  past an end normal passed, because the test used the same offset
  (`safety::s9_refuses_a_cut_past_an_open_path_end`).

(changed in #207: an open path could be cut only On line, so cutting
beside a line, as VCarve's Left and Right do, needed the line redrawn one
bit radius over. Issue #207's design comment is the source of these
rules; the strip rule was added in implementation, because a cut that
only keeps `r` from other lines still passes when it lies wholly on
another line's kept side, as a line drawn twice in opposite directions
shows. Changed in #311: a Left or Right Profile takes a last pass; see
A8, "Left and Right".)

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
   The excursion, descend point to end point, must keep `r` from every
   edge of the site's loops but the corner's own two, else
   `ReliefCornerTooTight` (`NearOtherWall`, #322): nearer, the notch
   cuts past that wall too, through a part thinner there than the notch
   is deep or into an island, which S9's notch rule refuses.
4. **Levels and the separate pass:** the levels are
   `generateStepLevels(top, bottom, tool.step_down)`; per corner and
   level the notch cuts from the descend point to the end and back. Only
   corners whose descend point the main path has cut at the deepest level
   are cut (else `ReliefCornerNotCut`). A tool with no step-down skips
   relief with `ReliefSkipped`. Since the maintainer decision of
   2026-10-24 the notches are cut inline (step 5). The separate pass
   below is the fallback (step 6): per corner and level, plunge at the
   descend point, cut to the end, cut back, with a retract between
   corners. (changed: maintainer decision 2026-10-24. The separate pass
   was the only way notches were cut.)
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
5. **Inline on the final wall (default).** Each level `ℓ` of a corner
   goes to the operation's first pass whose Z is at or below
   `ℓ + RELIEF_Z_TOLERANCE_MM` (the last pass if none). That pass's path
   has opened the column at the descend point `P` down to `ℓ` by the time
   it reaches `P`. A pass gets the levels between it and the pass above:
   one when the two step-downs are equal, several when the operation
   steps deeper than the tool, none when it steps finer.
   - **The final-wall path** of a corner at a pass is the block of the
     corner's shape at that pass that cuts the final wall, and that
     reaches `P` at the pass's Z within the guard (`RELIEF_POSITION_
     TOLERANCE_MM` plus the flatten tolerance):
     - Pocket: a closed ring of the corner's region, the wall ring or an
       island ring (`wall_pass: first` or `last`, and `allowance_mm` 0).
       Inner rings never reach `P`, and open raster lines never count.
     - Profile: the shape's lap. With a last pass (#284) that is the
       last pass's lap only. The allowance passes run on the allowance
       paths, which never get a notch.
   - **The detour** is #193's splice, on every pass with levels, not
     only the freeing one. Where the final-wall path first reaches `P` at
     its Z (a lap move that passes `P` in its middle is split there), the
     tool rises in the open column to the first of the pass's levels. At
     each level it cuts out to the end and back at the feed rate,
     stepping down at `P` at the plunge rate between levels. Then it
     returns to the pass's Z at `P` and carries on along the path. No
     lift, rapid or link is added. A corner's notch is cut where the path
     passes it, so the corners of one path follow the path's own order.
     A pass with no levels for the corner gets no detour.
   - **Step-down: no fallback.** The notch never steps deeper than the
     tool's step-down, whatever the operation's step-down. Every level is
     one tool step below the level before, and the material above each
     level in the lobe was cut by an earlier level. An operation that
     steps deeper than its tool cuts several levels in one detour, in the
     column its pass has just opened, as #193's freeing lap does. So a
     coarse step-down (a finishing Pocket or Profile cut in one full-depth
     pass) never cuts a notch as a full-depth slot, and needs no separate
     pass. That slot was PureCutCNC's reason to keep relief out of the
     wall path.
   - **Entries and leads.** The detour goes only into the block's cuts,
     after its descent. The plunge, ramp or helix descent never carries a
     detour, nor does a ramp along the wall that passes `P` on its way
     down (it is not at the pass's Z there). A lead-in or lead-out runs in
     the waste and meets the wall only at its join, so the first place a
     led lap reaches `P` at depth is on the lap. When the join is at `P`,
     the detour runs there. A path that starts at `P` runs the detour
     right after its descent. #348's staged descent moves are part of the
     lap, and a stage that passes `P` at the pass's Z counts.
   - **Tabs:** a corner under a tab fails the guard at the deepest level,
     since the tab lifts the lap over `P`, so it stays skipped with
     `ReliefCornerNotCut`. Tabs sit at the same place on every pass, so
     a corner that passes the guard is reached at depth on every pass.
   - **S9:** the detour's moves are the separate pass's moves (vertical
     at `P`, level from `P` to the end and back). They sweep the same
     lobe, so the notch rule and the generators' footprint lobes are
     unchanged.
   (changed: maintainer decision 2026-10-24. Every notch was cut by the
   separate pass of step 4: a Pocket's after all its passes, a Profile's
   after each pass's lap but the freeing one, lifting to safe Z and
   plunging at every corner and level.)
6. **Fallback to the separate pass** (step 4), only where there is no
   final-wall path to cut from:
   - A Pocket with `allowance_mm` above 0: its rings are allowance paths,
     not the final wall. Every corner is cut by the separate pass after
     the main passes, sized to the wall the allowance leaves, as before.
   - A Pocket corner where some pass that has levels for it has no
     final-wall path reaching `P` at its Z, for example a rest Pocket
     that skips the wall ring (#167): that corner, all its levels, goes
     to the separate pass after the main passes, so its levels are still
     cut top first. With `wall_pass: none` no ring reaches `P`. Every
     corner falls back, and the guard skips it with `ReliefCornerNotCut`
     unless a raster line happens to cut its descend point at depth, as
     before.
   - A Profile pass whose lap does not reach `P` at its Z: that pass's
     levels are the separate notch block in that pass's unit, after the
     lap (#189), as before.
   `ReliefSkipped`, `ReliefCornerTooTight` and `ReliefCornerNotCut`
   are given as before. Inline or not, a corner's notch is the same.
   (changed: maintainer decision 2026-10-24.)

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
outline"; a part always keeps one), and any outline on which it crosses
no placed tab ("Placed tabs"); the tabs hold the rest. (changed in #109: a tabbed Profile
freed nothing, because every outline had tabs.) (changed in #105: a tab
is a request's or a placed tab the Profile crosses, so a Profile with no
request is tabbed on the outlines that carry placed tabs.)
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

A Bore frees nothing (it leaves no core, "Bore" step 1) and its targets
are closed outlines, so as `j` it warns as a Pocket does: a Bore after a
cut-out of its part warns `CutInsideFreedPart`, and after a tabbed one
`CutInsideTabHeldPart` (changed: maintainer decision 2026-10-24).

The targets of `j` include the open paths it cuts: every open path of an
Engrave and of an On line Profile (Inside and Outside Profile, Pocket,
Bore and Drill skip them; one of no length is skipped). An open path is inside a
freed outline when all of it is inside or on it (every vertex and every
segment's middle, `Region::contains_point` on the outline's region), so
one that crosses the outline does not count, as crossing outlines do not.
An open path frees nothing. (changed in #206: O8 read closed targets
only, so an Engrave or an On line Profile of a line inside a freed part
ran in the loose part with no warning.)

The outlines a tabbed Profile through the stock cuts with tabs, a
request's or placed tabs it crosses, are held only by them
(`profile::cut_outlines`, `Hold::Tabs`). Every target of
`j`, closed or open, that lies inside one of them by the same rule
yields, when `j` cuts deep, `CutInsideTabHeldPart (op: j, earlier_op: i,
instance, held)`. `j` cuts deep when its `depth_mm` is more than half the
stock thickness (plus `EPS_MM`), which takes in every cut that reaches
the stock bottom, through-cut allowance and all; with either value unset
it does not (validation reports them). A lighter cut, such as a 1 mm
Engrave after a tabbed cut-out (the plaque's before maintainer decision
2026-10-24 moved its Engrave first), is normal practice and quiet. `held` is
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
   up to 2 quanta, else reject. The arcs as written (formatted ends, the
   centre from the written `I`,`J` or `R`) must then follow the moves they
   replace as S12 judges them, within the tolerance plus one unit in the
   last place, and cover at least three of those moves as S12 counts
   them, or the run is rejected. S12 counts neither a first move ending
   within a unit in the last place of the run's written start (read as a
   straight move written as nothing) nor any move after the first vertex
   that close to its written end. (changed in #355: the snap moved a
   centre 0.0072 mm in inch, two quanta each way, to agree on the radius
   to 0.0002 mm where the rounded words already agreed to 0.0013 mm, and
   the arc as written bowed 0.0126 mm off a chord the fitted arc bowed
   0.0099 mm off; S12 allows 0.0125 mm and refused it. A T-bone Pocket's
   run of two chords and a 0.0002 mm sliver, fitted as three, was two to
   S12, which refused it too.) Each run is judged
   from the emitted position, with the post's number format; a run with
   no known emitted start falls back (upstream judges it from its own start; this cannot
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
the initial Z, which is safe Z) once (changed in #305: at the clearance
height, when the project sets one and every link between holes is at it;
changed in #314: once per part when the links are at both heights; see
"As built (#283)"), then one line per hole: the command
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
In a Drill written as cycles, a toolpath rapid in Z alone may also end
partway along one rapid of the controller's in Z alone at its XY, when
the toolpath goes on the same way along that line by a rapid in Z alone,
or ends there going up (changed in #305: cycles from the clearance height
go straight from it to `R` and from the bottom back to it, where the
toolpath stops at safe Z). A series of cycles ends with `G80`: any other
motion, or a `G53` lift, while a cycle is the motion mode is
unparseable, and the next cycle after `G80` starts a new series from the
Z it is at (changed in #314: other motion also ended a series).
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
| Mach3 | Using Mach3Mill 1.84-A2; Mach3 Install & Config v3 | `%` (never numbered), no `O0001` (an `O` word is a subroutine label and "The O line may not have a line number", 10.5.2, 10.8.7); line numbers `N10` step 10 on every line but the `%`; `G90 G94 G17 G40 G49 G80`, `G91.1`, `G21`/`G20`, `G54` (changed in #286: version 2 has no `G49`; changed in #312: version 3 adds `G50` and `G69`, each on its own line, after the modal line, to cancel a `G51` scale and a `G68` rotation, which `M30` does not reset, Using Mach3Mill 1.84 10.7.15, 10.7.20, 10.8.1); `G4 P` **seconds** (the unit is a setting, shown as seconds in both manuals, and `G82 P` is documented in seconds); no `M6`; canned `G81`, `G82` only; footer `M5`, `M30`; `.tap` |
| UCCNC | UCCNC user's manual 1.0047 (software 1.2113) | no `%`, no `O0001`, no line numbers (the manual documents none of them); `G90 G17 G40 G49 G80 G94`, `G54` (changed in #286: version 2 has no `G49`; changed in #312: version 3 adds `G50` and `G69` after the modal line, user's manual 4.1.1 p. 58-60, since "Reset on file load G68 and G51" is only a checkbox, 3.6 p. 35); no `G20`/`G21` (not in the code list: lengths are in the profile's units, so the program has no units word and `export` warns `units_set_by_controller`); `G4 P` **milliseconds** (the default); no `M6`; no canned cycles; footer `M5`, `M30`; `.txt` |
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
  refused (#154). (changed in #312: `G50` and `G69` are written again
  after every pause too, and accepted only from a definition whose header
  has them, Mach3 v3 and UCCNC v3, alone on a line, in the header or the
  setup after a stop; anywhere else the export fails with
  `MisplacedSetupWord`. `G51` and `G68` are refused everywhere. LinuxCNC,
  GRBL and RS274/NGC have no such codes; grblHAL's `G50`/`G51` are
  #316.) (changed in #316: grblHAL v4's header has `G50`, so the same
  rule accepts it there; grblHAL has no `G69`, which stays refused.)

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
   - **Rounding** (changed: maintainer decision 2026-10-24; values were
     the raw `f64` product, such as `1524.0000000000005` for 60 in/min
     and `4.762499999999999` for 3/16"). A value converted from inches is
     rounded: lengths (`diameter_mm`, `step_down_mm`) to 6 decimal places
     of a millimetre, feeds (`feed_mm_min`, `plunge_mm_min`) to 3 decimal
     places of mm/min. `step_over_fraction`, which the import computes in
     either unit, is rounded to 6 decimal places, as are the millimetre
     values a `StepOverClamped` note states. A value in a `millimeters`
     file is not converted and is kept as written. Why 6 and 3: 1/64" is
     0.396875 mm, so 6 decimals keep every binary fraction of an inch to
     1/64" and every inch value with up to 4 decimals (0.0001" =
     0.00254 mm) exact, and a nanometre is far below any machine's
     resolution; a feed in in/min with up to 2 decimals (0.01 in/min =
     0.254 mm/min) is exact at 3, and programs write `F` with fewer.
     Rounding is `(v × 10^d).round() / 10^d` in `f64`
     (`dc_project::units_input::round_length_mm`, `round_rate_mm_min`):
     deterministic, and the result is the `f64` nearest the decimal, so
     the file saves `4.7625`, not `4.762499999999999`.
   - **Stored tools are not rounded on load.** A library or project
     number may be one the user typed in millimetres, which a load cannot
     tell from a converted one; rounding on load would also break the
     byte-identical round trip and silently change projects, which embed
     their copies (ADR-0002). A tool imported before the change keeps its
     values until it is re-imported with `tools import --replace <file>`,
     which overwrites it in place (same `source`, same id) with the
     rounded values and the vendor name. Re-importing a rounded tool is
     `already present`: rounding is deterministic.
4. **Presets:** one `Tool` per preset: name `description` for one preset,
   `"<description> (<preset name>)"` for several. A tool with no
   `description` takes its `product-id`, else `<DC> <mm|in> <type>` (for
   example `6.7 mm drill`; Autodesk's metric sample has 179 drills with
   no description). In the bull nose suffix, `RE` is written in the
   file's unit.
   - **Vendor** (changed: maintainer decision 2026-10-24; the name was
     the description alone, so two vendors' `1/4" Downcut` collided and
     IDC Woodcraft's ED-12 and ED-14 engravers imported as `Widget Works
     Diamond Engraver` and `… (2)`). `vendor` → `Tool::vendor` and
     `product-id` → `Tool::product_id`, each trimmed, absent or empty →
     `None`. With a vendor, the name before the bull nose and preset
     suffixes is `<vendor> <description>` (`IDC Woodcraft 1/4"
     Downcut`), or `<vendor> <product-id>` and `<vendor> <DC> <mm|in>
     <type>` in the fallbacks; when that part already starts with the
     vendor (compared ignoring case, and ending at a word boundary) it is
     not prefixed again. Without a vendor the name is as before.
     Clashes are settled when the tool is added to the library (see
     "Tool library CLI"), so a clash with a tool already there is
     handled the same way as one inside the file. `source = "fusion:<tool guid>:<preset guid or index>"` so
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
   (`imported`, `imported as <name> (<product id>)` or `imported as
   <name> (2)`, `replaced` or `already present`, then its notes, for example `` `5 mm Drill`: imported (lt6);
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

- `tools list [--json]`: id, name, vendor, product id, cutter, diameter,
  feeds, RPM, number, source. Columns `ID NAME VENDOR PRODUCT CUTTER
  DIAMETER_MM FEED_MM_MIN PLUNGE_MM_MIN RPM NUMBER SOURCE`, `-` when
  unset. PRODUCT fits: product ids are short (IDC Woodcraft's longest is
  `8675309-125DJ`, 13 characters), far narrower than SOURCE, and it is
  the part number a user reorders by and the suffix that tells clashing
  names apart. (changed: maintainer decision 2026-10-24: no VENDOR or
  PRODUCT column.)
- **Name clashes** (`dc_library::add` and `replace`; changed: maintainer
  decision 2026-10-24, the suffix was always ` (2)`): a tool whose name a
  different library tool already has is stored as `<name> (<product
  id>)` when it has a product id, the name does not already end with
  ` (<product id>)`, and no other tool has that name; else as `<name>
  (2)`, then ` (3)`, … as before. So ED-12 imports as `IDC Woodcraft
  Widget Works Diamond Engraver` and ED-14 as `IDC Woodcraft Widget Works
  Diamond Engraver (ED-14)`. "The same tool" ignores either suffix on the
  stored name, so importing the file again is `already present`. Lookups
  by name (`tools add`, `tools remove`) match the stored name exactly, so
  they take the new names.
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
dreamcarve tabs place <dcv> <op> [--count <n> | --at <mm>,...] [--width <mm>] [--height <mm>] [--shape rect|smooth] [--on-holes]
dreamcarve tabs list <dcv>
dreamcarve tabs clear <dcv> [<op> | --tab <id>...]
dreamcarve config path
```

`init` writes `arc_fit_tolerance_mm: 0.01`, the user config's
`tool_change_mode` into the post-processor, and one Profile as in phase 1.
It does not write Pocket, Drill or Engrave operations: the user adds them,
following the chapters, which each carry a complete JSON block to paste.
`export` and `check` run `order::check` after generation, then
`placed_tabs::check` (#105), and print their warnings with the others.

`tabs` (#105, `crates/dreamcarve/src/tabs.rs`; "Placed tabs"):

- `tabs place <dcv> <op> --count N`: `placed_tabs::place` with a
  `TabSpec` of `count` N and the flags (width 8, height 3, `rect`, no
  holes by default: `init`'s). `--at d1,d2,…`: `place_on_outlines`.
  Neither: the operation's own request, set to `null` (a usage error,
  exit 2, when it has none). Prints `Placed `tab5` on `coaster`
  (instance `i1`, profile 0) 35.343 mm along it: 8 mm wide, top 3 mm
  above the stock bottom, rect` per new tab, then `N tab(s) placed;
  coaster.dcv saved with M tab(s).`, and the placing warnings on stderr.
  An outline that had tabs and gets more is noted after the total:
  "Note: `coaster` (instance `i1`, profile 0) already had 4 tab(s);
  this adds 6 more. To swap them instead, give --replace."
  `--replace` (#293; needs `--count` or `--at`, exit 2 without):
  `placed_tabs::replace`; prints `Removed …` per tab removed (as
  `Placed …`), the `Placed …` lines, `R tab(s) removed and N placed;
  coaster.dcv saved with M tab(s).`, and the `tabs clear` note for each
  part it leaves cut through with no tab. (changed in #293: adding was
  the only way, so changing the count took `tabs clear` first.)
  With or without `--replace`, each new tab an enabled Profile cuts as a
  hole warns `tab_on_hole` once, on stderr (#300); a tab `--replace`
  removes and places again takes a new id, so it is new and warns again.
  With `--on-holes` (`--count` or `--at`) each tab placed on a hole is
  saved with `holds_slug: true`, and every other tab placed with `false`
  (#302).
- `tabs list <dcv>`: a table `ID INSTANCE PROFILE AT_MM OFF_MM X_MM Y_MM
  WIDTH_MM HEIGHT_MM SHAPE HOLDS_SLUG CUT_BY`, `AT_MM` the distance of the tab's
  foot along its outline from the first vertex as drawn, `OFF_MM` how far
  its point is from the foot in project space (`off_outline_mm`; changed
  in #300: the distance was not shown), `HOLDS_SLUG` the tab's
  `holds_slug` as `yes`, `no` or `-` for `None` (changed in #302: not
  shown), `CUT_BY` the
  enabled Profiles that cross it (`-` for none). Requests are shown as
  the tabs they place, with a note line; nothing is saved.
- `tabs clear <dcv>` removes every tab, `tabs clear <dcv> <op>` the
  tabs the Profile `op` crosses (`crossed_by`; another kind of operation
  fails, exit 1), `--tab <id>` (repeatable) the ones named; an unknown
  id fails (exit 1) and saves nothing. After removing, each part an
  enabled Profile through the stock now cuts with no tab
  (`placed_tabs::freed_parts`, compared before and after) is named on
  stdout: "Note: operation `op1` now cuts `coaster` (instance `i1`,
  profile 0) through the stock with no tab, so the part comes loose on
  its last pass." (changed in #105 review: `<op>` removed every tab on
  the instances it targeted, so clearing a finish pass or a Pocket freed
  the part silently.)
- `tabs place` refuses `--count 0`, and `--width`, `--height`,
  `--shape` or `--on-holes` without `--count` or `--at` (exit 2).
- `place` and `clear` first run `place_requests`, then save through
  `dc_project::save`, staged and renamed into place. An unknown
  operation fails with "operation `op9` is not in coaster.dcv", one that
  is not a Profile with "operation `o2` is a pocket operation; only a
  Profile cuts tabs" (exit 1). `export` prints, per program, the operations in
it and the warnings per operation, each as `<code>: <message>`.

## Module layout

```
crates/dc-geometry/src/{..., boolean.rs (i_overlay union/difference/intersection on Regions), circle.rs (circle test, Kasa fit), self_band.rs (a line against itself, #221)}
crates/dc-project/src/{model.rs (+ PocketOp, DrillOp, EngraveOp, Entry, Lead, CornerRelief, MachiningOrder, PlacedTab), ids.rs (+ TabId), migrate.rs (v7_to_v8, v8_to_v9, v10_to_v11), validate.rs (+ rules above), post_processor.rs (+ use_canned_cycles, SpindleDwell::units, CannedCycles::reentry_gap_mm), definitions/{README.md, grblhal, grbl, linuxcnc, mach3, uccnc, generic}/v1.json}
crates/dc-cam/src/{lib.rs, toolpath.rs (+ Dwell), block.rs (CutBlock), sequence.rs, order.rs, profile.rs (blocks; entry, lead, relief hooks), pocket.rs, pocket/{rings.rs, raster.rs, rest.rs}, drill.rs, bore.rs (format 18), engrave.rs, entry.rs, lead.rs, relief.rs, footprint.rs, passes.rs, tabs.rs, placed_tabs.rs (#105), overlap.rs, stock.rs, warning.rs}
crates/dc-gcode/src/{emit.rs (+ canned cycles, split modes), arcfit.rs, canned.rs (controller model of G81–G83/G73), safety.rs (+ S9–S15), parse.rs (+ G2/G3 fitted, G8x expansion, G4, `%`), post.rs (definition checks)}
crates/dc-import/src/{svg.rs, dxf.rs, dxf/{groups.rs, document.rs, entities.rs, spline.rs, stitch.rs}}
crates/dc-library/src/{lib.rs, library.rs, fusion.rs, error.rs}
crates/dreamcarve/src/{main.rs, cli.rs, init.rs, export.rs, check.rs, tools.rs, tabs.rs (#105), config.rs}
tests/golden/{pocket-offset-rect, pocket-offset-island, pocket-raster-0deg, pocket-raster-45deg, pocket-rough-then-rest, drill-simple, drill-peck, engrave-open-closed, profile-ramp, profile-helix-inside, profile-lead, profile-dogbone, order-full-depth-first, order-pass-first, post-linuxcnc-coaster, post-mach3-coaster, post-uccnc-coaster, post-generic-coaster}/
tests/fixtures/{plaque.svg, plaque.dxf, dxf/*.dxf, fusion/{sample.tools, sample.json}}
crates/dc-project/tests/fixtures/dcv/{v7, v8, v9, v10, v11}/sample.dcv
crates/dreamcarve/tests/fixtures/tabs-migration/*.dcv, crates/dreamcarve/tests/expected/tabs-migration/*.nc (#105)
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
- `distance::nearest_point_on_a_line_reports_distance_along`, `distance::nearest_point_on_an_arc_follows_the_sweep`, `distance::nearest_point_at_an_arc_centre_takes_the_arc_middle`, `transform::inverse_undoes_the_transform` (#105).

**dc-project**
- `migrate::v7_profile_gains_phase_2_fields_with_defaults` — fixture loads with `machining_order: full_depth_first`, `entry: plunge`, `lead: null`, `corner_relief: none`, `arc_fit_tolerance_mm: 0.01`, `use_canned_cycles: false`; `migrate::v7_to_v8_writes_the_defaults`, `migrate::v7_with_phase_2_field_already_present_fails`.
- `cli::v7_project_exports_the_same_program` (the spec's `v6_fixture_exports_phase_1_bytes_with_fitting_off` at version 8).
- `serialize::tool_number_round_trips`, `validate::duplicate_tool_number_is_error`, `validate::set_number_clashing_with_positional_is_error`, `validate::tool_number_zero_is_error`, `emit::set_tool_number_is_written_as_t_word_and_file_name`, `emit::unset_tool_number_falls_back_to_position`.
- `serialize::pocket_drill_engrave_round_trip`, `serialize::entry_variants_serialize_as_string_or_object`, `serialize::clearance_strategy_raster_carries_angle`.
- `migrate::version_9_fixture_loads`, `migrate::version_10_fixture_loads`, `migrate::v10_project_gains_an_empty_tab_list`, `migrate::v10_with_tabs_already_present_fails`, `serialize::placed_tabs_round_trip`, `validate::placed_tab_on_a_missing_outline_is_an_error`, `validate::placed_tab_size_and_point_are_checked` (#105).
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

**dc-cam — placed tabs (#105)**
- `placed_tabs::placed_requests_cut_what_the_requests_cut` (Outside both directions, an odd count with a tab on a corner arc, Inside at distances, On line, with a ramp and a lead: the same motion once placed).
- `placed_tabs::tab_no_enabled_operation_crosses_warns` (no Profile, only a disabled Profile, only a Pocket: `tab_not_crossed`; a crossed tab is quiet).
- `placed_tabs::tab_follows_its_instance_when_it_moves`, `placed_tabs::two_profiles_on_one_outline_both_rise_over_its_tabs`, `placed_tabs::tabs_of_different_heights_rise_each_to_its_own_top`.
- `placed_tabs::place_keeps_the_rules_for_holes_and_short_outlines`, `placed_tabs::tab_on_a_hole_is_cut_and_warns`, `placed_tabs::place_does_not_place_a_tab_twice`, `placed_tabs::place_needs_a_profile_of_the_project`, `placed_tabs::place_on_outlines_puts_a_tab_at_each_distance`.
- `profile::star_with_tabs_placed_only_on_the_star_lifts_only_on_the_star` (acceptance 38a).
- `safety::placed_tab_programs_pass_s1_to_s9`; the golden harness checks that every golden case with tabs cuts the same once its tabs are placed.
- `cli::version_9_tab_settings_export_byte_identical_programs` (the tabs-migration corpus, as loaded and after `tabs place`), `cli::init_asks_for_four_tabs_that_tabs_place_writes`, `cli::tabs_place_list_and_clear`, `cli::tabs_place_at_puts_tabs_along_the_outline`, `cli::tabs_place_refuses_what_it_cannot_place`, `cli::check_warns_tab_not_crossed`.
- `placed_tabs::replace_swaps_only_the_tabs_the_profile_crosses`, `cli::tabs_place_replace_swaps_the_operations_tabs`, `cli::tabs_place_without_replace_notes_existing_tabs` (#293).
- `placed_tabs::tab_far_from_its_outline_warns`, `placed_tabs::hole_tabs_placed_for_the_cut_do_not_warn`, `cli::tabs_list_shows_how_far_each_tab_is_off_its_outline`, `cli::placed_v8_hole_tabs_export_without_tab_on_hole`, `cli::tabs_place_replace_onto_a_hole_warns_once` (#300).
- `migrate::v14_placed_tabs_migrate_with_holds_slug_unset`, `migrate::v14_with_holds_slug_already_present_fails`, `migrate::v14_with_holds_slug_says_where`, `migrate::holds_slug_round_trips`, `migrate::invalid_holds_slug_says_where`, `placed_tabs::hand_placed_hole_tab_still_warns`, `placed_tabs::v14_hole_tabs_on_the_path_hold_their_slugs`, `placed_tabs::place_at_distances_on_holes_marks_the_hole_tabs`, `placed_tabs::hole_tabs_hold_their_slugs_off_corners_with_a_last_pass`, `cli::tabs_place_at_on_holes_exports_without_tab_on_hole`, `cli::tabs_place_count_on_holes_holds_slugs`, `cli::tabs_list_shows_whether_each_tab_holds_its_slug`, `cli::hand_edited_holds_slug_is_refused_with_its_place`, `cli::version_14_projects_export_byte_identical_programs_and_warnings` (#302; the issue's `migrate::v12_hole_tabs_on_the_path_hold_their_slugs` became the `null` migration and `placed_tabs::v14_hole_tabs_on_the_path_hold_their_slugs`, since a migration step cannot see tool paths).

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
- `profile::open_path_left_offsets_to_the_left`, `profile::open_path_right_offsets_to_the_right`, `profile::left_right_on_closed_target_is_skipped_with_warning`, `profile::left_right_ramp_and_tabs`, `profile::left_right_cut_into_the_kept_side_is_refused`, `profile::left_right_tool_too_large_for_a_tight_arc_warns`, `offset::open_offset_moves_left_of_travel_and_ends_square`, `offset::open_offset_rounds_outer_turns_and_trims_inner_ones`, `offset::open_offset_of_a_tight_arc_toward_its_centre_vanishes`, `safety::left_right_cut_off_its_path_fails_s9`, `cli::left_profile_of_an_svg_line_exports_on_its_left` (#207). `profile::left_climb_keeps_the_part_on_the_right_of_travel`, `profile::right_climb_keeps_the_part_on_the_right_of_travel`, `profile::left_conventional_is_the_reverse` (#287, replacing #207's `profile::left_right_direction_follows_vcarve`). `side_last_pass::open_path_short_end_segment_keeps_the_kept_strip` (the issue's `profile::open_path_short_end_segment_keeps_the_kept_strip`, in dc-gcode because it reads the emitted G-code), `safety::s9_refuses_a_cut_past_an_open_path_end`, `profile::left_right_end_segment_with_no_room_is_refused`, `profile::left_right_cut_into_the_strip_past_an_end_is_refused`, `offset::strip_ends_run_along_the_end_normals_to_the_first_line` (#342).
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

## Phase 2 addendum (#73)

**Status:** draft. The design of the capabilities in `functional.md`,
"Phase 2 addendum (#73)" (A1–A9, capabilities 27–36, acceptance 39–47,
S16). Everything above still holds; each subsection says what it changes.

### Order and format versions

One issue per subsection, built in this order. Three of them add
persisted fields, so each bumps the `.dcv` format version once, and only
one agent bumps it at a time: #207 takes version 10 and #105 version 11,
then A7, A8 and A9 take 12, 13 and 14 in that order. Each bumping issue
starts only after the issue holding the version before it has merged. If
#105 has not merged when A7 is picked up, the maintainer decides whether
A7 takes 11; the versions after it shift by one and this table is
updated in the same PR. The other six change no persisted field.

| Item | Issue | Model change | `.dcv` version | Depends on |
| --- | --- | --- | --- | --- |
| A1 `export --post` keeps settings | #277 | none | — | — |
| A2 Comment transliteration | #278 | none | — | — |
| A3 Header modal state | #279 | definitions `grblhal/v3`, `grbl/v3` | — | A1 (the upgrade it prompts keeps settings) |
| A4 Cycle-time estimate | #280 | user config `[estimate]` | — | — |
| A5 SVG toolpath preview | #281 | none | — | — |
| A6 Inch input | #282 | reader only | — | — |
| A7 Separate heights | #283 | `Project::heights`; definition field `work_coordinates.machine_command` | 12 | #105 (v11), A3 |
| A8 Profile last pass | #284 | `ProfileOp::last_pass` | 13 | A7 |
| A9 Tab corners, start point | #285 | `TabSpec::avoid_corners`, `ProfileOp::start` | 14 | A8; A5 helps check it |
| (follow-up) Hole tabs hold their slugs | #302 | `PlacedTab::holds_slug` | 15 | A9 |
| (follow-up) Tool vendor and product id | maintainer decision 2026-10-24 | `Tool::vendor`, `Tool::product_id`; library version 2 | 16 (or the next free one) | #302 |
| (follow-up) Skip Drill holes narrower than the tool | maintainer decision 2026-10-24 | `DrillOp::allow_tool_wider_than_hole` | 17 | tool vendor (16) |
| (follow-up) Bore operation | maintainer decision 2026-10-24 | `Operation::Bore(BoreOp)` | 18 (or the next free one) | Drill holes (17) |

A definition's own version (`grblhal/v3`) is separate from the `.dcv`
version and follows `definitions/README.md`, "Versions". A3 adds the only
new definition versions; A7's definition field is a schema-wide field,
added in place to every released version (README, "Versions").

### A1. `export --post` keeps settings (dreamcarve::export, dc-project)

`dc-project::post_processor` gains

```rust
/// The latest version of `id` with `old`'s settings carried over by the
/// rule in functional capability 27.
pub fn switch_post(old: &PostProcessorRef, id: &str) -> Result<(PostProcessorRef, Vec<SettingReset>), PostProcessorError>;
pub struct SettingReset { pub setting: &'static str, pub from: String, pub to: String, pub reason: ResetReason }
pub enum ResetReason { NoM6, NoCannedCycles }
```

For each of the six settings: if `old`'s value equals the default of the
definition version `old` names, take the new version's default;
otherwise keep it, unless it is invalid on the new definition (the same
rules `validate` applies: `"m6"` needs an `M6` line, `use_canned_cycles`
needs `canned_cycles`), in which case take the new default and record a
`SettingReset`. The CLI applies the config's `tool_change_mode` only to a
setting the rule set from a default or reset. `--reset-settings` (only
with `--post`) keeps today's path: `PostProcessorRef::latest` plus
`config::seed_post`. The summary prints `Updated <dcv>: post-processor
grblhal v1 → v3, keeping your settings.` and one line per reset.

Tests (`dc-project`): `post_processor::switch_keeps_changed_dwell`,
`switch_takes_new_default_for_default_extension` (grblHAL `nc` →
LinuxCNC `ngc`), `switch_keeps_changed_extension`,
`switch_resets_m6_without_m6_line` (grblHAL `"m6"` → GRBL, with
`ResetReason::NoM6`), `switch_resets_canned_cycles_without_them`,
`switch_same_id_changes_only_version`. CLI: `cli::export_post_keeps_dwell`
(acceptance 39), `cli::export_post_reports_reset_setting`,
`cli::export_post_reset_settings_restores_defaults`,
`cli::export_post_config_applies_only_to_reset_settings`.

### A2. Comment transliteration (dc-gcode::ascii)

`sanitize_comment` (`emit.rs`) maps each character through
`ascii::transliterate(c) -> Option<&'static str>` before its existing
filter. The table is DreamCarve's, written by hand and reviewed, with no
new dependency: Latin-1 Supplement U+00A0–U+00FF and Latin Extended-A
U+0100–U+017F (`é` → `e`, `Ø` → `O`, `ß` → `ss`, `Æ` → `AE`, `Œ` → `OE`),
the General Punctuation dashes, quotes and ellipsis U+2010–U+2026 (`—` →
`-`, `“` → `"`, `…` → `...`), and a few symbols a workshop writes: `°` →
`deg`, `×` → `x`, `±` → `+-`, `µ` → `u`, `²` → `2`, `³` → `3`, `¼` `½`
`¾` → `1/4` `1/2` `3/4`, `™` → `TM`. No entry maps to `( ) ; ! ? ~ { } %`
(a test checks the whole table), and unmapped characters are dropped as
today. The length cap counts the transliterated text. `deunicode`
(BSD-3-Clause) was considered: its table covers scripts whose output
nobody here can check, and the few hundred entries above cover the names
the review saw. The rule is the emitter's, so it applies to every
definition version; it changes comment bytes only, for names that hold
such characters, and CHANGELOG lists it under "Changed".
`definitions/README.md` ("Verification", the emitter rules paragraph) is
updated to say so.

Tests (`dc-gcode`): `ascii::table_never_produces_comment_specials`,
`ascii::latin1_letters_lose_accents`, `ascii::fractions_and_symbols_spell_out`,
`emit::comment_text_transliterates` (acceptance 40),
`emit::comment_text_drops_unmapped`, and the existing
`emit::comment_text_is_sanitized` and `comment_text_is_truncated`
unchanged.

### A3. Header modal state (dc-project/src/definitions, grblHAL v3 and GRBL v3)

`grblhal/v3.json` and `grbl/v3.json` are version 2 of each with two
header lines added after `G94`: `G80`, then `G40`. Added to `SOURCES` and
pinned; `init` and `export --post` take them; v1 and v2 are unchanged. The
emitter already writes the header's modal lines again after every stop
(#55), in header order, and writes a motion word on the first move after
setup, which `G80` requires. `definitions/README.md` gets a section
"Header modal state (`grblhal/v3.json`, `grbl/v3.json`)" with this
table. Sources: `grblHAL/core` at
`4ec1ae44b857a70d2f9298344feac4b43ed0eb6b` and `gnea/grbl` at
`bfb67f0c7963fe3ce4aaf8a97f9009ea5a8db36e` (v1.1h), the commits the
README already cites; line numbers are at those commits.

| Code | grblHAL | GRBL | Decision |
| --- | --- | --- | --- |
| `G80` | `gcode.c` 1592-1597: sets motion mode `None` and clears `canned_cycle_active`. Canned cycles `G73`, `G81`–`G89` exist (1613-1620), so a cycle mode can be left by MDI or by a program stopped without `M2`/`M30` (program end clears it, 5031-5033). Its only error is an axis word on a line in `G80` mode (3335-3339); the header line has none, and the next motion line carries `G0`/`G1`. | `gcode.c` 169-171 with `gcode.h` 77 (`MOTION_MODE_NONE 80`): sets motion mode none. GRBL has no canned cycles (group 1 `G81`–`G89` is under "Not supported", `gcode.c` 1137, 1150), so it cancels nothing there; its only error is the same axis-word rule (646-649). | Added on both. Harmless on both; useful on grblHAL, and a GRBL program also runs on grblHAL. |
| `G40` | `gcode.c` 1662-1668: always accepted, "since it often appear in g-code program headers to setup defaults". With `CUTTER_COMP_ENABLE` (default `Off`, `config.h` 226-227) it turns compensation off, and "A standalone G40 is modal-only" (2852); in a block while a canned cycle is active it fails with `Status_CutterCompConflict` (2856-2861), so it is written after `G80`. Program end also turns compensation off (5039-5043). | `gcode.c` 203-208 and 483-486: accepted and does nothing, "only for the purpose to not error when G40 is sent with a g-code program header". | Added on both, after `G80`. |
| `G49` (cancels `G43.1`) | `gcode.c` 1670-1677 and 969-971: sets the tool length offset to 0. The offset survives program end (group 8 is not reset, 5023-5026) and a soft reset (`gc_init` 789-793 keeps it), and `$341` modes 1–3 set one after a touch-off (`tool_change.c` 81, 345-346). But `G10 L20`, which sets work zero, subtracts the active offset: "WCS = MPos - G92 - TLO - WPos" (`gcode.c` 3095-3098). | `gcode.c` 209-224, 984-993: sets the offset to 0. It survives program end (1092-1095) but not a reset (`gc_init` 42-44 clears all state). `G10 L20` and `G92` subtract it (550-553, 568-571). | **Not added on either.** If the offset was active when the user set Z zero, `G49` moves every Z by it, down when it was positive. The offset is part of the zero the user set, so the program leaves it alone and `export` says so (functional 29). |
| `G43`, `G43.2` | Need a tool table (`gcode.c` 1678-1681; `N_TOOLS` default 0, `config.h` 548). | Unsupported (`gcode.c` 220). | Not added (and DreamCarve writes no tool table, as for the slice 17 definitions). |

`export` prints the tool-length line of functional capability 29 for any
program whose definition id is `grblhal` or `grbl`.

**Open question for the maintainer (not part of A3).** The LinuxCNC,
Mach3 and UCCNC headers (slice 17) carry `G49`. LinuxCNC's touch-off is
also `G10 L20` from the current position, which includes an active `G43`
offset, so the same shift applies there if a user zeroes Z with an offset
active. DreamCarve's documented workflow (set Z zero with each bit, no
`G43`) avoids it, and those headers follow each controller's documented
preamble; whether to keep `G49` there needs its own check against each
controller's `G10 L20`.

Tests (`dc-gcode` `post` and `emit`): `post::grbl_v3_header_cancels_modes`
(v3 = v2 plus `G80`, `G40` in that order after `G94`, for both),
`post::no_header_cancels_tool_length_offset` (no `G49`/`G43` in any
grblHAL or GRBL version), `emit::setup_after_stop_repeats_g80_g40`,
`emit::first_motion_after_g80_has_motion_word`,
`post::released_definition_versions_are_unchanged` (v3 pinned),
`cli::export_grblhal_prints_tool_length_note`,
`cli::v2_project_exports_v2_bytes` (acceptance 41).

(changed in #316: `grblhal/v4.json` is v3 with `G50` on its own line
after `G40`, to cancel a `G51` scale, which program end does not reset
(`gcode.c` 5021-5053). `G50` is in every build (1766-1770, 2739-2800,
not behind an option; `MACH3_SCALING` only changes how `G51` reads its
words). It moves nothing and changes no offset (220-239, 4695). It fails
with cutter compensation on (2742-2745), so it comes after `G40`. It
takes the block's axis-command slot (1767), so it is not put on a modal
line. `init` and `export --post grblhal` take v4. v1–v3 are unchanged,
and v3's coaster is kept as `tests/expected/coaster-grblhal-v3.nc`.
`grbl` stays at v3: GRBL has no `G50`. grblHAL has no `G68`/`G69`
(1781). At this commit `set_scaling` (226) reports a change only for X,
so `G50` leaves a scale with X at 1 (`G51 Z-1`) in force (4475-4482). It
is written anyway: it is harmless and clears every scale that touches X.
The post-processors chapter tells the user to soft-reset (`gc_init`
822) and check `$G`. README, "Scaling on grblHAL". Tests:
`post::grblhal_v4_header_cancels_scaling`,
`post::scaling_and_rotation_words_only_where_supported`,
`emit::setup_after_stop_repeats_g50_g69`, `safety::g50_g69_accepted`,
`safety::grblhal_v4_g50_only_where_emit_writes_it`,
`cli::grblhal_v3_project_still_exports_byte_identically`,
`cli::grblhal_v3_project_upgrades_to_v4`.)

### A4. Cycle-time estimate (dc-gcode::estimate)

```rust
pub struct Estimate { pub feed_s: f64, pub dwell_s: f64, pub feed_mm: f64, pub rapid_mm: f64, pub stops: usize }
pub fn estimate(program: &Program, post: &PostProcessor) -> Result<Estimate, ExportError>;
```

It walks the moves the safety parser reads from the program text, so it
times what is written (fitted arcs, canned-cycle expansions, dwells):
`G1` length and `G2`/`G3` length (arc length, with its Z change, as a
helix) at the modal `F`; `G4 P` in the definition's dwell units; `G0`
3D length, untimed; `M0`, `M1`, `M6` counted as stops. Program units are
converted to mm. Adapted from PureCutCNC's `feedControlledTimeSeconds`
(`src/engine/operationBooklet/report.ts`), which times feed moves and
reports rapid distance apart; that file is the attribution source. The
CLI reads `[estimate]` from the user config (`rapid_mm_min`: finite and
above 0; `scale_factor`: finite and above 0, default 1.0; an invalid
value is an error naming the file, line and column, as for other keys)
and prints the line of capability 30 per program and a total. This
widens "Only `init` and `export --post` read the config": `export` also
reads `[estimate]`, for the printed estimate only, never for a program.

Tests: `estimate::line_time_is_length_over_feed`,
`estimate::arc_time_uses_arc_length`, `estimate::helix_time_includes_z`,
`estimate::dwell_respects_definition_units` (UCCNC milliseconds),
`estimate::inch_program_converts`, `estimate::rapids_are_distance_only`,
`estimate::stops_are_counted`; CLI `cli::export_prints_estimate`
(acceptance 42), `cli::estimate_with_rapid_rate`,
`cli::estimate_scale_factor_multiplies`,
`cli::estimate_config_does_not_change_program`,
`config::estimate_rejects_non_positive_rate`.

### A5. SVG toolpath preview (dreamcarve::preview)

```rust
/// A deterministic SVG of the project's stock, shapes and each
/// enabled operation's toolpath, in mm, Y flipped for display.
pub fn render(project: &Project, ops: &[(&Operation, &Tool, &Toolpath)]) -> String;
```

In the CLI crate, the only user until the phase 4 GUI draws toolpaths
itself. Ported from PureCutCNC's `src/engine/designPrint/svg.ts`
(`buildStock`, `buildFeatures`, `buildToolpaths`, `fmt`, `escapeXml`),
with the upstream header; the page scaffolding is dropped.

- **Frame.** `width="<W>mm" height="<H>mm" viewBox="0 0 W H"`, the stock.
  ADR-0001: the renderer is where the flip happens, `y_svg = H − y`, done
  on each coordinate (no `transform`), so the file reads in drawing
  terms. Numbers have three decimals; no date or random id, so the bytes
  depend on the project only.
- **Content.** The stock rectangle; each instance's profiles, thin
  black; per enabled operation a `<g id="<op id>">` in a colour from a
  fixed 8-colour palette by operation index. Cutting moves as one path
  per run between rapids; `Linear` as `L`, arcs as `A r r 0 large sweep x
  y` with sweep 0 for `ArcCcw` and 1 for `ArcCw` (Y-up CCW looks CCW
  after the flip, which is SVG's negative direction), split at 180°.
  Rapids dashed grey. A level cutting move above the lowest Z of its
  run is "over a tab": drawn again with a heavy stroke. The first point
  of each run's descent gets a circle of the bit's diameter and a tick
  one diameter long along the first cutting move. A Drill's plunges are
  circles of the bit diameter. A legend lists op id, name and colour.
- **CLI.** `--preview <file.svg>` on `export` (staged and committed with
  the programs, all or none) and on `check` (written when every
  operation generated; a `.dcv` path or a program path is refused).

Tests (`dreamcarve` unit and CLI): `preview::flips_y_once`,
`preview::ccw_arc_has_sweep_zero`, `preview::arc_over_180_is_split`,
`preview::tab_marks_raised_level_moves`, `preview::start_circle_per_run`,
`preview::escapes_names`, `preview::is_deterministic`,
`cli::export_preview_coaster` (acceptance 43),
`cli::check_preview_writes_file`, `cli::export_preview_refuses_dcv_path`.

### A6. Inch input (dc-project::units_input)

A serde helper for every persisted length and rate: `#[serde(deserialize_with
= "units_input::mm")]` on each `f64` field named `*_mm` (and
`units_input::opt_mm` on `Option<f64>`), `units_input::mm_min` on
`*_mm_min`, in `model.rs` (and so in the tool library, which stores
`Tool`). A JSON number is taken as is. A JSON string must match `<number>
<unit>`, whitespace optional: `<number>` is a decimal (optional sign),
`a/b`, or `w a/b`; length units `mm`, `in`; rate units `mm/min`,
`in/min`. The value is `n × 25.4` (`in`) or `n`, an `f64` product, so
loading is deterministic. An `in` or `in/min` value is then rounded to 6
decimal places (lengths) or 3 (rates), as Fusion import rounds ("Fusion
import", step 3), so `"3/16 in"` is `4.7625`; an `mm` string and a bare
number are kept as written. (changed: maintainer decision 2026-10-24:
the product was not rounded. No program changes: programs write fewer
decimals.) Any other string is a serde error, which the
existing locator reports with the field, line and column. Validation
then runs on the millimetre value as before. `save` is unchanged, so a
saved file has numbers. Fields with other units (`angle_deg`,
`step_over_fraction`, `rpm`, `dwell_s`) take numbers only.

No format bump: ADR-0002's version describes what DreamCarve writes,
which does not change. A file with a unit string, read by an older
DreamCarve, fails at that field with a located type error, as any
malformed value does.

The CLI's summary formats a length with the project's `display_units`
and that unit's `decimal_places`; the extents already use the program's
units.

Tests (`dc-project`): `units_input::inch_string_converts_exactly`,
`units_input::fraction_and_mixed_number`, `units_input::rate_in_per_min`,
`units_input::bare_number_is_mm`, `units_input::unknown_unit_is_located`,
`units_input::length_unit_on_rate_field_is_error`,
`serialize::save_writes_numbers_after_unit_input`; CLI
`cli::inch_input_exports_same_program` (acceptance 44),
`cli::inch_project_summary_in_inches`.

### A7. Separate heights (format version 12)

```rust
pub struct Project { /* … */ pub heights: Heights }   // after `origin`
pub struct Heights {
    /// Above the stock top, mm. Links between instances, between operations, tool changes, the end.
    pub clearance_mm: Option<f64>,
    /// Machine Z for `G53 G0 Z`, mm (written in program units). Before every tool-change stop and at every program end.
    pub tool_change_machine_z_mm: Option<f64>,
    /// End position, project coordinates.
    pub park: Option<ParkXy>,
}
pub struct ParkXy { pub x_mm: f64, pub y_mm: f64 }
// definition schema, `work_coordinates`:
pub machine_command: Option<String>,   // "G53" or null
```

**Migration v11 → v12** (`migrate::v11_to_v12`): add `heights` with three
`null`s after `origin`; fails if the key is there. Fixture
`crates/dc-project/tests/fixtures/dcv/v11/sample.dcv`; its program is
byte-identical after migration. `init` writes the three `null`s.

**Definition field.** `work_coordinates.machine_command` is added in
place to every released version: `"G53"` on grblHAL v1–v3 and GRBL v1–v3,
`null` on LinuxCNC, Mach3, UCCNC and generic v1, all re-pinned. No bytes
change, since nothing writes it unless the project sets the height. The
README cites, for grblHAL, `gcode.c` 1543-1545 (`G53` is non-modal group
0) and 3317-3325 (it needs `G0` or `G1` in the block, and fails with
cutter compensation on); for GRBL, `gcode.c` 154-156 and 635-640 (the same
two rules). Both move to the given machine coordinate ignoring work and
tool offsets. The homed envelope is negative: grblHAL
`machine_limits.c` 116-140 (with hard limits on, the top is
`−pulloff`), GRBL `limits.c` 382 (Z homes to `−pulloff`) and `system.c`
348 (soft limits allow `[max_travel, 0]`). So the validation rule is
`tool_change_machine_z_mm < 0`, and the chapter says to stay at least the
pull-off below 0. LinuxCNC v1 and v2 are `"G53"`, set in place and
re-pinned (changed in #306: they were `null` because their documents had
not been checked). The README cites the 2.9 interpreter at `a33293f`:
`interp_array.cc` 109 (group 0), `interp_check.cc` 103-108 (`G0` or
`G1`, absolute), `interp_convert.cc` 4855-4857 (no cutter compensation),
`interp_find.cc` 170-200 with `get_abs_position` (no work, `G92` or tool
length offset), and 2749-2830 (program units); and for machine Z,
`emctaskmain.cc` 2302-2303 (no program before homing, by default),
`motion/command.c` 1011-1025 (a move past a soft limit is refused before
it runs) and stepconf's and pncconf's Z travel of −100 to 0 mm. The
validation rule and S16 are unchanged. Mach3, UCCNC and generic stay
`null`, with the reason in the README: their manuals do not say what
`G53` does with a tool length offset (UCCNC: nor with `G51` scaling), and
machine Z after homing is a user setting there; generic cannot be checked.

**Validation** (errors): `clearance_mm` below any enabled operation's
`safe_z_mm` (`Problem::ClearanceBelowSafeZ`, at the field, naming the
operation); `tool_change_machine_z_mm` ≥ 0 or not finite
(`Problem::MachineZNotBelowHome`); set on a definition whose
`machine_command` is `null` (`Problem::MachineZNeedsG53`, naming the
post-processor); `park` not finite or beyond the ±6000 mm bound (#75).

**Sequence.** `SequenceInput` gains `clearance_z_mm: f64` (the clearance
height in project space, or `safe_z_mm` when unset). A link between
blocks of different instances retracts to it; a link within one
instance (passes, holes, notches) to `safe_z_mm`; stay-down links inside
cleared material are unchanged (O3).

**Emitter.** The retract before an operation's first move, after its
last, and around a tool change goes to the clearance height. Before the
first stop of a tool change (the router-off pause, or `M5` and `T<n> M6`)
and before the footer, when `tool_change_machine_z_mm` is set, it writes
`<machine_command> G0 Z<v>` in program units, marks motion modal `G0` and
work Z unknown. At the end, before that lift: rapid at the clearance
height (or the last operation's safe Z) to `park` if set. XY moves before
the lift, at a work Z the safety pass can check, rather than after it.
`export` prints `Before you start: home the machine; this program moves to
machine Z <v> (G53). Z must home upward, or soft limits must be on
($20=1).` when the program has a lift (changed in #308: the soft-limits
sentence added). The README's "Tool
changes" paragraph "The tool-change height is not set" is replaced.

**Safety** (`dc-gcode::safety`, `parse`): S16 as in the functional spec;
the parser reads `G53 G0 Z` lines and refuses any other `G53`; S6 accepts
the clearance rises and the end sequence; S7 is unaffected (the park
rapid is not a cut).

Tests: `migrate::v11_to_v12_adds_null_heights`,
`migrate::version_11_fixture_loads`,
`validate::clearance_below_safe_z_is_error`,
`validate::machine_z_must_be_below_zero`,
`validate::machine_z_needs_g53_definition`,
`sequence::links_between_instances_rise_to_clearance`,
`sequence::passes_of_one_instance_link_at_safe_z`,
`emit::tool_change_lifts_to_machine_z_before_m6`,
`emit::program_ends_at_park_then_machine_z`,
`emit::machine_z_written_in_inch_units`,
`emit::unset_heights_are_byte_identical`,
`safety::g53_only_z_alone`, `safety::xy_rapid_after_g53_needs_retract`,
`safety::g53_refused_without_height`, `post::machine_command_cited`,
`cli::heights_plaque_one_program` (acceptance 45),
`cli::export_prints_home_note`.

**As built (#283).** Where the build departs from or adds to the text
above:

- The end height. With a park point and no clearance height, the program
  rises to the highest safe Z of its operations, not the last one's:
  motion after the last operation is in no operation's span, where S2
  holds it to that height (changed in #283: the text said the last
  operation's safe Z).
- Same-tool operations. Only the first operation and a tool change write a
  retract at their start; an operation that follows one with the same tool
  starts where that one's rise to the clearance height left the bit. The
  first move in XY after a retract from an unknown position runs at the
  retract's height before it lowers, so an operation's first crossing is at
  the clearance height (`Writer::retracted_z`).
- The lift and the stop. When the program has lifted with `G53`, it writes
  no retract in work coordinates before the stop (the retract to the next
  operation's height came first), and the safety pass refuses any motion
  between a lift and the stop or end it is for
  (`ExportError::MachineLiftNotBeforeStop`). Two stops may follow one lift
  with nothing moving between them (manual spindle control: the router-off
  pause, then `T<n> M6`). It also refuses a lift from below safe Z
  (`MachineLiftNotAtSafeZ`) and one to another machine Z than the
  project's, or to 0 or more (`MachineLiftWrongZ`).
- Canned cycles. A Drill whose holes are separate instances links them at
  the clearance height, which `G98` cannot follow, so with a clearance
  height above safe Z it is written as moves with
  `CannedExpansion::ClearanceHeight`. Holes of one instance keep cycles.
  (Changed in #305: such a Drill is written as cycles from the clearance
  height; see the next item. Only a Drill whose links are at both
  heights is still written as moves with that reason. Changed in #314:
  that Drill is written as a series per part; see the item after next.)
- Canned cycles from the clearance height (#305). `G98` returns each hole
  to the Z before the first cycle of the series (LinuxCNC 2.9
  `interp_cycles.cc` 926-931 and 976, the initial level `cycle_il`, reset
  by any motion that is not a cycle, `interp_convert.cc` 3818-3819; [G]
  `#gcode:g98-g99`; Mach3Mill 10.7.29, whose "just before the canned
  cycle started" gives the same height in a `G98` series, since each line
  starts where the one before returned). So when the project sets a
  clearance height and every link between holes is at it (holes that are
  separate parts, or one hole), `canned_plan` takes it as the initial Z:
  the rapid over the first hole is at the clearance height, where the
  operation starts, and `G98` brings every hole back to it. Links at safe
  Z keep safe Z as the initial Z, as before; links at both fail the
  comparison (`ClearanceHeight`). The controller then rapids from the
  clearance height straight to `R` (preliminary motion, [G]
  `#gcode:preliminary-motion`, Mach3Mill 10.7.24.1) and from the bottom
  straight back up, where the toolpath stops at safe Z both ways. The
  toolpath is unchanged; `expansion_is_toolpath` and S6 instead take
  rapids along one vertical line in one direction as one rapid
  (`join_rapids`; `safety::match_rapid_on_the_way`), only for a Drill
  written as cycles, so plain programs are held to every move as before.
  Tests: `emit::canned_cycles_at_the_clearance_height` (the plaque on
  LinuxCNC, `G81` and `G82`),
  `emit::canned_motion_from_the_clearance_height_equals_plain_program`
  (`G81`, `G82`, `G83`, `G73` on the test definition, the same pecks),
  `emit::canned_cycles_with_the_clearance_height_at_safe_z`,
  `emit::canned_cycles_from_the_clearance_height_with_the_lift_on_linuxcnc`
  (with #306's `G53` lift: `G80`, the park, the lift, the footer),
  `emit::clearance_height_expands_canned_cycles` (links at both heights),
  `safety::canned_cycles_from_clearance_match_the_toolpath`,
  `safety::vertical_rapids_join_only_along_a_cycle_rapid`.
- A series per part (#314). A Drill linked at the clearance height
  between parts and at safe Z between the holes of one part is written as
  one `G98` series per run of holes linked at safe Z. Each series is a
  rapid over its first hole at safe Z, `G98`, its cycle lines (the code,
  `X Y Z R`, `Q`/`P` and `F` on the first line again), and `G80`; between
  series the tool rises with `G0 Z<clearance>`, crosses at it and comes
  down to safe Z. `G80`, like any motion that is not a cycle, resets the
  initial level (LinuxCNC `interp_convert.cc` 3818-3819 in
  `convert_motion`, which runs `G80` too; [G] `#gcode:g98-g99`), and a
  new series needs its `Z` and `R` again (`interp_cycles.cc` 729-734,
  920-923; Mach3Mill 10.7.24, sticky numbers). Every line of every series
  starts at safe Z, so Mach3Mill 10.7.29's "just before the canned cycle
  started" gives safe Z however it is read. The motion is the plain
  program's move for move: the toolpath already stops at safe Z on the
  way down from the clearance height, so nothing is joined. `G99` is not
  used: it returns to `R`, below safe Z. `canned_plan` splits the holes at
  each link at the clearance height (`CycleLines::series`), and
  `expansion_is_toolpath` adds the rise after a series and the descent
  to safe Z before the next. Links at any other height keep the fallback
  (`ClearanceHeight`); a Drill from `generate_drill` never has them. The
  parser refuses any motion or `G53` lift while a cycle is the motion
  mode, so S11 never relies on a series ended without `G80`, which
  Mach3Mill does not document. Tests:
  `emit::canned_series_per_part_at_the_clearance_height` (`G81`, `G82`,
  `G83`, `G73` on the test definition: four series, crossings at Z 20
  between parts and at Z 5 within the pair, the same pecks, the plain
  program's motion), `emit::canned_series_per_part_on_linuxcnc_and_mach3`
  (mm and inch, and with #306's lift on LinuxCNC),
  `emit::clearance_height_expands_canned_cycles` (links at a third
  height), `safety::series_per_part_match_the_toolpath` (refuses a series
  without its `G80`, one from the clearance height, and `G99`),
  `parse::a_series_of_cycles_ends_with_g80`.
- Corner relief. A Pocket's notches, sequenced one corner at a time after
  its main path, are reached at the clearance height when the corner is of
  another instance than the one before, and the first one always (where
  the main path ended is not known there).
- Validation. A non-finite machine Z is `NonFinite`, as every other field;
  the clearance height and the machine Z are also held to the ±6000 mm
  bound (#75), and the clearance height must be positive.
- The note. `export` writes the machine Z with its units, `Before you
  start: home the machine; this program moves to machine Z -5.000 mm
  (G53).`; with one program per tool run, once, as "each program moves".
  `Program::machine_lift` carries it.
- Review of #304. (1) With manual spindle control the router-off pause
  follows the lift and the user may jog there, so the program lifts again
  right before `T<n> M6`; S16 accepts a lift right after a stop that
  followed a lift with nothing moved since, once the units are set again
  (S5). (2) Validation and the emitter refuse a machine Z that rounds to
  0 in the program's decimal places. (3) The safety pass takes the
  clearance height (`safety::ProgramHeights`): with it set, each
  operation's first XY rapid, every XY rapid outside the operations (the
  park move) and every stop are at or above it
  (`ExportError::RapidBelowClearance`, `PauseNotAtSafeZ`). Links between
  parts inside an operation are not told apart from links between passes
  in the program text, which does not say which part a move is over (a
  pocket's island is a part of its own, and its raster ends lie on it);
  S6 holds them to the toolpath, whose links the sequencer writes and
  `sequence::links_between_instances_rise_to_clearance` checks. (4) A
  program with a machine Z has the header comment `( Lifts to machine Z
  with G53: home the machine before running. Needs Z to home upward, or
  soft limits on with $20=1 )`; others are unchanged (changed in #308:
  the soft-limits sentence added).
- Soft limits (#308). On GRBL or grblHAL with Z homing downward
  (`HOMING_FORCE_SET_ORIGIN`, grblHAL "force set origin"), every negative
  machine Z is below the travel: with soft limits on the controller
  alarms before the lift moves, with them off the lift drives Z down. The
  export note and the header comment name both conditions. DreamCarve
  cannot read the homing direction or `$20`, so it is a note, not a
  refusal. Header bytes change only for a program with a machine Z, so
  no definition version is added. The wording is per controller, since
  `$20` is GRBL's: on LinuxCNC (#306), whose soft limits always apply once
  homed and which runs no program unhomed unless `[TRAJ] NO_FORCE_HOMING`
  is set, the header says `Soft limits apply only once homed: leave
  NO_FORCE_HOMING unset` and the note `LinuxCNC applies soft limits only
  once homed: leave [TRAJ] NO_FORCE_HOMING unset.`
  (`dc_gcode::machine_lift_limits_note`). Tests:
  `emit::machine_z_header_comment`,
  `emit::machine_z_header_comment_on_linuxcnc`,
  `cli::export_machine_z_note_names_soft_limits`,
  `cli::export_machine_z_note_on_linuxcnc`.
- The estimate (A4). A `G53` lift has no work-coordinate motion, so
  `dc-gcode::estimate` leaves its distance, and that of the rise after it
  from an unknown Z, out of the rapid distance.
- The v11 corpus. `crates/dreamcarve/tests/fixtures/heights-migration/`
  holds the 16 tabs-migration projects saved at version 11 by the build
  before #283 (`tabs place`); `cli::version_11_projects_export_byte_identical_programs`
  exports each, as loaded and saved at version 12, against the
  tabs-migration expected programs.

### A8. Profile last pass (format version 13)

```rust
pub struct ProfileOp { /* … */ pub last_pass: Option<LastPass> }  // after `corner_relief`
pub struct LastPass {
    pub allowance_mm: f64,          // (0, D/3]
    pub thickness_mm: Option<f64>,  // (0, depth_mm]; None = the normal last pass
    pub reverse: bool,
}
```

**Migration v12 → v13**: `last_pass: null` on every Profile. Fixture
`dcv/v12/sample.dcv`. `init` writes `null`.

**Validation** (errors): `allowance_mm` not in (0, D/3]
(`Problem::LastPassAllowance`, naming the bit diameter and the limit);
`thickness_mm` not in (0, depth]; `last_pass` on `Side::OnLine`
(`Problem::LastPassNeedsSide`). (changed in #284: Left and Right are
`LastPassNeedsSide` too, as capability 34's "Inside and Outside only"
says; all three `LastPass` keys are required, as `Lead`'s are; the
generator refuses the same values, `CamError::InvalidLastPass` and
`CamError::LastPassNeedsSide`. Changed in the #310 review: `thickness_mm`
above the step-down in effect, the operation's or else the tool's, is
`Problem::LastPassThickerThanStepDown`, located at the thickness and
naming the step-down's field, so a last pass is never thicker than a
pass; and a cut of only one pass, whose allowance nothing leaves, warns
`last_pass_unused`. Changed in #311: Left and Right take a last pass, so
`LastPassNeedsSide` is On line only, `Side::has_wall`.)

**Algorithm** (`dc-cam::profile`).

1. **Depths.** With `thickness_mm = t`, `pass_z_mm` runs to `depth − t`,
   then one pass at `depth`; without, the usual depths.
2. **Paths.** Each outline gets two tool-centre paths: the final path,
   at `r` as now, and the allowance path, at `r + a` on the waste side
   (the same offset call with the larger distance). Passes but the last
   cut the allowance path; the last cuts the final path, in the opposite
   direction when `reverse`. The band and narrowness checks
   (#192, #199, #223) run with `D + 2a` for the allowance path, so a band
   that fits the bit but not the allowance is refused, naming it.
   (changed in #284: the checks are `dc-cam::profile::last_pass::twins`.
   Inside, an allowance path within `r + a` of another line of its
   region is `CamError::BandNarrowerThanAllowance`. Where the outline is
   wider than `D` but narrower than `D + 2a` somewhere, the allowance
   offset loses or gains loops (a neck, a small hole, an inlet's mouth):
   `CamError::ShapeNarrowerThanAllowance`. An allowance path within `r`
   of a line of another region, or (Outside) inside another region's
   material, is `BandNarrowerThanAllowance` too, its needed gap `D + a`:
   S9 holds only a lap at the radius to D6's warning, so such a pass
   would fail the safety pass. Allowance paths join D6's overlap check,
   its needed gap grown by `a`. A cut of one pass has no allowance path:
   its one pass is the last. Changed again in the #310 review: the last
   pass removes at most what its cutter reaches past the allowance
   passes', which is at most the farthest its path runs from theirs; that
   must be at most `√2 · a + 0.05 mm`, a square corner's, or
   `ShapeNarrowerThanAllowance`. This replaces the earlier bound of `r`,
   which let a notch, slot or tip the earlier passes skip be cut whole at
   full depth; it refuses every path corner sharper than 90°, at any
   allowance, since there the last pass takes `a / sin(θ/2)`. Changed in
   #324, `last_pass::cuts_only_corners`: the part path is sampled every
   `min(r/8, 0.25)` mm and at each vertex, with its distance to the
   allowance path. Nowhere may it be farther than
   `max(D/3, √2 · a + 0.05)`. A *departure* is a stretch farther than
   `a + 0.05`; one that reaches past `√2 · a + 0.05` must be a corner:
   its turning, summed between samples, is one way (at most 1° the other,
   for sampling and flattening) and less than 179°. Otherwise it is
   `ShapeNarrowerThanAllowance`. Why: a corner's earlier passes cut both
   its walls and stop `a / sin(θ/2)` short of the last pass, which takes
   only the wedge between. D/3 is the thickest skin `check` lets the
   allowance be on a straight wall, so the wedge is never thicker than a
   skin the last pass may already take at full depth; `√2 · a + 0.05`
   keeps everything #310 cut. A stretch that turns back, or as far as a
   U-turn, goes into a notch, slot or inlet the allowance path bridges:
   the earlier passes never enter it, and the last pass cuts it whole,
   about as wide as the bit. Thickness alone cannot tell the two apart:
   a 7 × 3 mm notch cut outside at 0.5 mm with a 6.35 mm bit takes 1.0 mm
   off, as a 60° corner does. Measured on a 0.05 mm material lattice
   against the earlier passes' cuts (`profile::last_pass_allows_sharp_corners`),
   the last pass's widest removal is the wedge within the lattice: at
   0.5 mm, 0.98 mm in a 60° triangle, 1.28 mm in a 45° one, 1.36 mm at a
   42° star tip, 0.95 mm in a 60° V notch. Its momentary engagement as it
   meets the next wall's skin in any inside corner, square ones
   included, is not what this bounds; #310 allowed that too.)
3. **Tabs** are laid out on the final path (the tabs left on the part).
   On the allowance path each tab stretch is the nearest-point projection
   of the final stretch's ends, widened by `a` each way, so the raised
   stretch covers the same tab. (changed in the #310 review: widening by
   arc length let the allowance passes cut into tabs at sharp corners,
   where the allowance path runs past the corner beside the next edge's
   tab. `last_pass::twin_tabs` finds the tab's material instead, the stock
   the last pass leaves standing above its floor, on a lattice of `r/12`
   within the radius of its raised cuts, and raises each allowance-path
   point to the highest of it within `r + r/12`, each raised stretch
   `r/12` longer at both ends; rect if any tab under it is, else smooth,
   its ramps no longer than keeps it at or above that height, which holds
   at every earlier pass's Z.)
4. **Entry and lead** are planned per path, each in its own domain; the
   last pass descends where the allowance passes have already cut to
   `depth − t` (its XY is within `a < r` of their path), so S2's rapid
   down stops 1 mm above that floor. (changed in #284: S2's floor counts
   a cut only within `CLEARED_POINT_TOLERANCE_MM` of the rapid's XY, and
   the final path is `a` from the allowance path, so the rapid stops 1 mm
   above the stock and the plunge at the plunge rate runs down the
   cleared column to `depth − t`, then the entry; a helix, which has no
   bore of the earlier passes to rejoin, helixes from 1 mm above the stock
   instead. S2 is unchanged. The allowance path's domain is the waste less
   the part grown by `r + a`; a reversed last pass's helix turns the other
   way. Changed in #339, `profile::last_pass_block` and
   `last_pass::enter_from_twin`: that plunge went through the allowance's
   uncut strip, which stands to the stock top beside the part, so it ran
   up to the full depth into material. Of the issue's two options this takes the
   first, the configured entry from the earlier passes' floor: a staged
   straight plunge at the last pass's own XY can never start in cleared
   material, since the strip lies under the cutter there at every depth.
   The last pass's block is built from the block one more pass of the
   allowance path would be at the last depth, its descent by the entry
   from the previous pass Z where that pass ended: a plunge one step-down
   (or `t`) deep; a helix rejoining its bore; a ramp along the allowance
   path itself (`PathEntry::Along`, its run cut short before a tab as On
   line's), in the kerf every earlier pass cut, so it never leaves it (a
   led allowance path keeps the entry planned at its staging point, which
   its earlier passes descended at). Then, at or below the Z the last
   pass's lap starts at (a tab's height, #337), the allowance pass's cut
   on to its point nearest the last pass's start (its lap start, or its
   lead's staging point); straight up there to that Z if lower; and one
   step at the feed to the last pass's start. (Changed in the #339 review: below the tab top
   that was the domain less the tabs' entry keep-outs, the allowance
   path's widened stretches grown by `r`, which is right for a ramp or
   helix but refused a 0.5 mm step on the far side of a hole from its tab:
   318 projects main cut, every one with a ramp, helix or lead and tabs.
   The step is now held to what it sweeps: no nearer any line of the
   operation than the bit's radius, or than its ends already are (the
   entries' waste domain also leaves out every other region's kerf, so
   it refused a small part's step inside a hole), and above the tab
   material the last pass leaves,
   `last_pass::tab_clear_z_mm`, the highest that material stands within
   the radius of the step on an `r/12` lattice, so a step beside a tab
   rises over it. A step that leaves the waste would be a defect,
   `OffMaterial`.) That point is at most
   `max(D/3, √2·a + 0.05) + 0.05` mm away, the bound
   `cuts_only_corners` holds every point of the part path to. A part
   path whose seam is free (no lead, no explicit start) is re-seamed at
   its point nearest the allowance path's start, so the step is the
   allowance and nothing of the allowance pass is cut again; an
   allowance path is led in at its point nearest the part path's lead
   arrival (`lead_in_beside`), so with the same arc the step joins the
   staging points `a` apart. A last pass lead-in whose staging point is
   farther is left off with `lead_dropped` (`no_room`), the step going
   to its lap start; no step that fits is `ShapeNarrowerThanAllowance`.
   Measured on the emitted G-code with a 0.1 mm heightfield
   (`last_pass_entry::last_pass_entry_never_plunges_deeper_than_step_down`,
   12 mm in 4 mm steps with a 6.35 mm bit): before, the deepest plunge
   into uncut material was 12 mm with a plunge entry, 8 mm with a ramp
   (11 mm with `thickness_mm` 1); after, 4 mm, the step-down, in every
   case. Where the last pass's lap starts on a tab (#337), the step runs
   at the height the lap starts at; if the allowance path's cut is
   higher there (a smooth tab's slope), at its height, and the tool comes
   down at the start onto the slope, no lower than the lap cuts there.
   A tab taller than the step-down still has the last pass come down its
   height in one move there and at a rect tab's far end: #348.)
   (Changed in #348, `profile::tab_descent`: the earlier passes never cut
   the allowance strip, so wherever the last pass comes down it stands
   under the cutter at the height the lap last cut it, and a straight drop
   there goes its whole height into uncut material. Of the issue's
   options this takes the staged descent: ending the allowance path's
   raised stretches where the last pass's end cannot help, since in the
   cases triage measured (#353, #358) those stretches are already the
   shorter, and the strip is uncut whatever they do. Refusing or warning
   would not fix it.
   *A rect tab's far end* (`stage_tab_descents`): every straight drop of
   the last pass's lap and overcut deeper than the step-down in effect
   is cut in stages, as #339 holds every other plunge into uncut
   material. It stops at each level of `pass_z` between the tab top and
   the drop's foot, the floors the earlier passes cut (a gap wider than
   the step-down, which the passes never leave, is split evenly), so the
   first stage goes down to the floor of the pass before and each one
   after a pass level at most. After each stage but the last the tool
   runs level, at that stage's Z, along the lap's own moves after the
   drop for one tool radius (less if the level run is shorter; an arc
   keeps its centre and its feed), to that point or back to the drop, so
   the stages alternate between the two; after the last it runs back to
   the drop if it is not there, and the lap goes on as before. Every
   stage then plunges at most a step-down below a layer just cut, and
   every level move lies on the lap's path where the lap cuts at the
   drop's foot, below it, so it touches no tab. Every entry gets the
   same stages: a rect tab's lift and drop are straight moves whatever
   the entry, as without a last pass. *A lap that starts on a smooth
   tab's slope* (`onto_slope`, where the step runs higher than the lap
   starts): of the issue's two options this enters at a point of the lap
   at least as high as the step. An open path's lap must start at its
   end (#311), so moving the start off the tab as #176 does is no option
   there, and a closed lap starts there only by the user's start point.
   From the step's end the tool runs level at the step's Z along the
   lap's moves to where its slope rises to that height (on a closed lap
   that starts on a tab's far slope, backwards along the lap's end), then
   follows those moves back down to the lap's start, the descending ones
   at the feed scaled to the plunge rate, as the lap cuts its own far
   slopes. It never runs below the lap there, so into no tab. With one
   tab height the slope always rises to the step's height, which is no
   higher than the tab top; where it would not, the drop is kept.
   Measured on the emitted G-code with a 0.1 mm heightfield
   (`last_pass_entry::last_pass_entry_with_tall_tabs`: the 60 mm line
   Left and Right and the 24 × 16 mm rectangle Inside and Outside, with
   8 × 6 mm rect and smooth tabs, plunge and ramp, `reverse` off and on,
   12 mm in 4 mm steps with a 6.35 mm bit): before, every rect case and
   the line's smooth ones dropped 6 mm into uncut material; after, 4 mm,
   the step-down, and no tab material is lost. The #358 case
   (`properties::cases::inside_last_pass_helix_tabs_pass_first_exports`,
   a 12.96 mm tab in 1.44 mm steps) dropped 10.02 mm; after, at most the
   step-down.)
5. **Relief** is planned on the final path, as now; its notches are in
   waste beyond the allowance. (changed in #284: every relief level is
   cut from the last pass, the only one whose lap opens the column at a
   notch's descend point; an allowance pass is `a` off it.)
6. The last pass is the block that `frees`; `pass_first` orders by pass
   index as now.
7. **Feed round inside arcs** (added in #335, `last_pass::EdgeFeed`,
   `profile::own_edge_feeds`). The tool centre runs an arc of `ρ` round a
   wall of `ρ + r`, so the cutting edge moves `(ρ + r) / ρ` times as fast
   as the centre, and the last pass, meant to take a thin skin, takes
   several times a straight wall's in a minute. Of the issue's options
   this lowers the feed: a warning alone leaves the bit overloaded, and
   more radial cuts lower what each takes but not how fast the edge
   moves. Every level arc of the last pass's lap and overcut that turns
   round the wall (its centre on the side of travel away from the wall:
   the wall is on the right climbing, the left conventional, the other way
   reversed) and runs at the cutting feed runs at `feed · ρ / (ρ + r)`.
   The issue's `k` is 1, so the edge moves at the feed. That scale is the
   edge speed's, the conservative one: it also brings the material
   removed per minute per mm of depth to `feed · (a − a² / 2(ρ + r))`,
   under a straight wall's `feed · a`, where the removal's own scale,
   `ρ / (ρ + r − a/2)`, would leave the edge faster. Lead arcs, straight
   cuts, outside arcs (where the edge is slower than the centre), the
   ramps and lifts over tabs, and the earlier passes, which cut a full
   slot as without a last pass, keep the feed. A level arc over a tab's
   top is slowed like the rest of the lap: it still cuts the allowance
   above the tab. (Changed in the #335 review: this said every move over
   a tab kept the feed, which the code never did.) The scale is never below `a / (a + r)`, the
   edge's in the smallest hole the last pass cuts (`D + 2a`, its centre
   on a circle of `a`): S3 accepts a last pass's level cut down to
   `feed · a / (a + r)` and no slower, so the generator cannot invent a
   feed. A tighter inside arc, a fillet under `r + a` that the allowance
   path turns as a corner, runs at that floor, and its edge moves faster
   than the feed: `Warning::LastPassEdgeFaster` names the shape, the arc's
   radius, its feed and the edge's multiple of the feed (one per shape,
   the tightest). The emitter writes a level cut slower than the tool's
   feed, not at the plunge rate, rounded down in the last written place
   (#325's rule), so as written the edge moves no faster; no program
   without a last pass has such a cut, so they are byte-identical. Curves
   drawn as straight segments are not slowed (#350). Measured on the emitted
   G-code (`last_pass_entry::last_pass_small_hole_load`, 0.05 mm
   lattice, at a layer the earlier passes left the allowance on, every
   entry): before, in the 8 mm hole the edge moved at 4.85 times the feed
   and the lap removed 3.6 (0.5 mm) to 4.1 (0.2 mm) times a straight
   wall's share per minute, 2.0 to 2.5 in the 10 mm hole, 1.2 to 1.6 in
   the 15 mm; after, the edge moves at the feed and the removal is 0.69
   to 0.95 of a straight wall's.
8. **Curves drawn as straight segments** (added in #350,
   `last_pass::EdgeFeed::flattened_bend`, `profile::own_edge_feeds`). The
   importers flatten a DXF `ELLIPSE` or `SPLINE`, or an SVG Bézier that is
   not a circle, to short lines, which item 7 does not slow, so the edge
   there moves faster than the feed. The maintainer chose to warn, not
   slow down, in phase 2: one vertex cannot tell a flattened bend from a
   sharp corner, which #324 leaves at the feed, and the rectangle's last
   pass must not change. Arc fitting or a radius-based slowdown is a later
   phase's. The last pass's lap and overcut (as item 7, without its lead
   arcs) is split into runs of level straight cuts at the cutting feed;
   an arc, a tab's ramp or lift, or any other feed ends a run. Each vertex
   of a run turns by its signed angle, counted positive round the wall
   (to the left with the wall on the right) and negative away from it,
   and only if it turns less than **30°**; a vertex that turns more counts
   0. Over a window from the middle of one segment to the middle of a
   later one, the fewest segments reaching one bit diameter `D` (a run
   shorter than that is taken as `D` long), the tool centre's radius is
   `ρ ≈ L / Σθ`, and the edge moves at `(ρ + r) / ρ` times the feed.
   Where that is over **1.5**, the result carries
   `Warning::LastPassFlattenedCurve`, one per shape, its tightest bend:
   the shape, `ρ`, a vertex in the bend's middle and the ratio. Feeds and
   G-code are unchanged, so every golden is byte-identical.

   *Why 30°.* A curve flattened to a tolerance `s` turns `√(8s / R)` per
   vertex on a wall of radius `R`: 30° only under 0.3 mm at the default
   0.01 mm, or 2.9 mm at 0.1 mm, ten times coarser. So the vertices of
   every flattened curve tight enough to warn count, even at a coarse
   tolerance (at 20°, a 10 mm hole flattened at 0.1 mm, whose vertices
   turn 23°, would not warn, though its edge moves 2.7 times the feed).
   A rectangle's corners (90°) and a hexagon's or an octagon's (60°,
   45°) do not count, and #324 keeps deciding those; a 12-gon's 30°
   corners do not either.

   *Why 1.5.* Over 1.5 the tool centre's radius is under `2r`: the path
   turns more than a radian in one bit diameter, which takes at least two
   counted vertices, so a lone kink in a straight wall never warns (one
   under 30° spread over `D` is `ρ > 3.8r`, a ratio under 1.27; two
   within a diameter just reach 1.52). With a
   6.35 mm bit it is any wall bend under `3r`, 9.5 mm radius; the bit
   takes up to 50% more than its feed was set for there. Under that, the
   estimate's own spread (a window's few percent) and the margin feeds are
   chosen with would make the warning noise on every large spline. Item 7
   warns tight true arcs at any excess, but those are exact; this one is
   an estimate. Measured (`profile::last_pass_flattened_hole_edge_feed`
   and a sweep of flattened circles): circles flattened at 0.01 or 0.1 mm
   estimate within 3% of the true `R / (R − r)` (2.7 in a 10 mm hole, 1.7
   in a 15 mm, 1.5 in a 19 mm, none from 20 mm); the 12 × 8 mm ellipse
   at 0.5 mm warns at about 1.9; the 60 × 40 mm rectangle, Inside and
   Outside, true-arc holes, and hexagons to 24-gons 20 mm across do not.

S9 in the safety pass is unchanged: the allowance passes keep more than
`r` from the part, which it already accepts. The generator tests hold
them to `r + a`. (changed in the #310 review: "the part" in S9 is every
target region, the operation's other parts included, and the generator
holds the allowance passes to `r` from those, and outside them. Between
`r` and `r + a` of another part an allowance pass cuts only that part's
skin, which its own last pass removes, and is allowed.)

**Left and Right** (added in #311; VCarve's Last Pass Allowance applies
to its Profile of an open vector cut to the left or right, the A8 page;
PureCutCNC has neither). A Left or Right Profile cuts one flank of the
bit against the part, so it takes a last pass as Inside and Outside do.

1. **Side.** The allowance path is the open path offset by `r + a` to
   the cut side, `offset_open(path, ±(r + a))`, the same call as the
   last pass's with the larger distance (`last_pass::open_twins`). It
   runs the way the last pass's path runs (`open_runs_reversed`, the
   #287 spindle rule), so the earlier passes and the last mill alike.
2. **Ends.** Both offsets end square on the normals through the path's
   end vertices (an end segment with room for `r` but not `r + a` is
   `EndSegmentHasNoRoom` with the allowance, #342: the allowance path
   would run past the normal), so the earlier passes end where the last
   pass ends:
   neither is shortened nor extended. No pass reaches the part side past
   an end (the cutter's edge touches the end vertex only on the last
   pass, and stays `a` off it on the others); the earlier passes leave
   the `a` skin standing up to each end normal and the last pass takes it
   off there. Shortening the earlier passes would leave the last pass a
   full-width cut at each end; extending them would cut past the end.
3. **Narrowness and corners.** The pieces of the two offsets must pair
   one for one (an arc, U or hairpin on the cut side that fits the bit
   but not the bit plus the allowance makes the larger offset vanish or
   split), and the last pass is held to `cuts_only_corners` as on an
   outline, scanned from the path's start to its end with no wrap; a
   departure that reaches an end of the path is no corner, so it may be
   no thicker than a square corner's. Either is
   `ShapeNarrowerThanAllowance`. So the thickness cap
   `max(D/3, √2·a + 0.05)` applies to a V cut inside its tip.
4. **Neighbours.** `check_kept_sides` runs on the allowance paths with
   the reach `r + a` (the kept edges stay `r` off their lines): an
   allowance path within `r + a` of another line of the operation, or of
   any line's kept edge, would take that line's skin, which its own last
   pass is to cut, or cut its kept strip. That is
   `CamError::AllowanceCutsIntoKeptSide { op, shape, other, diameter_mm,
   allowance_mm }`, the counterpart of Inside's `D + 2a` band rule. Two
   lines cut towards each other need `D + 2a` between them; a cut towards
   another line's kept side needs `3r + 2a`. Lines that meet at an outer
   corner keep exactly `r + a` and are cut. Closed targets are skipped,
   so they are no neighbours.
5. **Tabs** are laid on the last pass's path (`PathEnds::Open`, from the
   end beside the first vertex) and carried onto the allowance path by
   `last_pass::twin_tabs`, its raised stretches cut short at the path's
   ends (`PathTabs::from_spans` with `closed: false`); a smooth stretch
   an end cuts short has no ramp at that end and holds the tab's top to it
   (changed in the #311 review: its ramp there was nearly zero long, so
   the pass fell to its own depth inside the tab in the last fraction of
   a millimetre). `twin_tabs` also
   samples each raised cut's reach all round it, just inside it
   (`RING_INSET_MM`): where the last pass rises onto a tab, the tab's
   foot at the far side of its kerf is a cusp thinner than the `r/12`
   lattice, which the allowance pass, `a` nearer that side, reaches up to
   `√(2ar − a²)` before the rise. This applies to outlines too, whose
   rises it lengthens by at most a lattice step.
6. **Entries, leads, relief.** A pass whose lap starts on a tab
   descends no lower than the tab there (#337, which this relies on: a
   reversed last pass, or an allowance pass, can start on a tab at a
   path's end). Each pass ramps along its own path
   (`entry::along_path`); a helix stays `HelixNeedsWasteSide`. No lead
   and no relief, as without a last pass. The last pass plunges through
   the column the earlier passes cleared, as on an outline (step 4).
   (Changed in #339: as on an outline, the last pass goes down on the
   allowance path, ramping along it, and steps across on the end normal;
   with `reverse` it goes down on the allowance path run backwards
   (`Polyline::reversed`, `PathTabs::reversed_open`), at the end where
   the earlier passes ended, so the tool does not retract.)
7. **`reverse`** runs the last pass's path backwards (`Polyline::reversed`)
   from the other end, where the earlier passes end, with its layout
   measured from there (`PathTabs::reversed_open`), so the tabs stay put
   and the milling mode flips, as it does on an outline.
8. **S9** (`Material::SideProfile`): with a last pass the open paths
   offset by `r + a` to the side are tool-centre paths too; every cut
   still keeps `r` from every open path and kept edge. (Changed in #339:
   so is the last pass's step across the band between the two offsets,
   `AllowanceBand`: a level move from the `r + a` offset to the `r` one,
   through points whose distances to them add up to `a`, no longer than
   `dc_cam::last_pass_step_reach_mm`, the generator's own bound. A move
   along the band, steps chained inside it, or one that falls through it
   is refused.) (Changed in #358: a point of the step may also lie
   between one segment's two offsets taken whole, untrimmed, its
   distances to them adding up to `a`. Where an open path's end vertex
   comes back within `r + a` of another of its segments, `offset_open`
   trims each offset of that segment where it comes within its own
   distance of the vertex, so the two passes start on circles of `r`
   and `r + a` round it, not on one normal, and the step's end lies
   farther than `a` from the trimmed `r + a` path. Between one segment's
   whole offsets a point is abreast of that segment on its cut side,
   `r` to `r + a` from it: the strip its passes cut, not past an open
   end and not on the kept side; the `r` S9 keeps from every line holds
   it off the vertex. The generator is unchanged, so the last pass still
   cuts all of its own path; starting both passes on one normal would
   leave the start of its path uncut.)

Tests: `profile::last_pass_left_offsets_earlier_passes`,
`profile::last_pass_left_right_tabs_cover_same_stretch` (the rise is
`√(2ar − a²)`, the #310 review's rule, not the issue's "widened by a"),
`profile::last_pass_left_right_tabs_keep_off_the_tab_material`,
`profile::last_pass_left_right_reverse_runs_from_the_other_end`,
`profile::last_pass_left_right_refuses_what_the_earlier_passes_cannot_follow`,
`profile::last_pass_left_right_allows_sharp_corners`,
`validate::last_pass_left_right_is_valid`,
`safety::left_right_last_pass_passes_s9`, the G-code heightfield checks
`side_last_pass::left_right_last_pass_keeps_the_kept_side_and_cuts_to_the_ends`
`side_last_pass::left_right_last_pass_keeps_the_tabs` and
`side_last_pass::left_right_last_pass_keeps_tabs_at_the_ends` (`dc-gcode`),
and `cli::left_right_last_pass_exports_and_without_is_byte_identical`,
whose projects without a last pass export the programs the build before
#311 wrote. The Left and Right property projects get random last passes.
#358 adds
`material::allowance_band_tests::last_pass_step_between_starts_trimmed_by_the_end_vertex_is_allowed`,
`material::allowance_band_tests::last_pass_step_past_an_open_end_or_on_the_kept_side_fails`
and the property case `left_open_path_last_pass_plunge_exports`.

Tests: `migrate::v12_to_v13_adds_null_last_pass`,
`validate::last_pass_allowance_limit`, `validate::last_pass_on_line_is_error`,
`validate::last_pass_thickness_range`,
`profile::last_pass_allowance_offsets_earlier_passes` (acceptance 46),
`profile::last_pass_thickness_adds_final_depth`,
`profile::last_pass_reverse_flips_direction`,
`profile::last_pass_tabs_cover_same_stretch`,
`profile::last_pass_band_too_narrow_for_allowance`,
`profile::last_pass_inside_rectangle`, `profile::no_last_pass_is_byte_identical`,
`safety::last_pass_program_passes_s9`; and (#339) `profile::last_pass_uses_the_configured_entry`
and the G-code heightfield checks (`dc-gcode`)
`last_pass_entry::last_pass_entry_never_plunges_deeper_than_step_down`,
`last_pass_entry::last_pass_entry_keeps_the_tabs` and
`last_pass_entry::no_last_pass_programs_are_unchanged`; and (#335)
`profile::last_pass_small_hole_edge_feed`, `profile::last_pass_tight_inside_arc_warns`,
`safety::s3_accepts_a_last_pass_edge_feed`,
`emit::last_pass_edge_feed_is_written_rounded_down` and the heightfield check
`last_pass_entry::last_pass_small_hole_load`; and (#350)
`profile::last_pass_flattened_hole_edge_feed`; and (#348)
`profile::tab_descent::tests`, the heightfield check
`last_pass_entry::last_pass_entry_with_tall_tabs` and the plunge depth of
`properties::cases::inside_last_pass_helix_tabs_pass_first_exports`.

### A9. Tabs off corners and start point (format version 14)

```rust
pub struct TabSpec { /* … */ pub avoid_corners: bool }   // init true; migration false
pub struct ProfileOp { /* … */ pub start: StartAt }      // init and migration "auto"
pub enum StartAt { Auto, FirstVertex, Near { x_mm: f64, y_mm: f64 } }  // "auto" | "first_vertex" | { "near": { … } }
pub const TAB_CORNER_TURN_DEG: f64 = 30.0;              // dc-cam::tabs
```

**Migration v13 → v14**: `avoid_corners: false` at the end of every
non-null `tabs`, `start: "auto"` after `last_pass` on every Profile.
Fixture `dcv/v13/sample.dcv`, byte-identical program.

**Corners** (`dc-cam::tabs`, after `Tabs::fit` places the tabs and before
the coverage rule of #109). The turn of a stretch of path is the sum of
the absolute tangent turns in it: each vertex's turn plus each arc's
sweep. A position is on a corner when some window one bit diameter long,
inside the tab's stretch widened by half a diameter each side, turns at
least `TAB_CORNER_TURN_DEG`. Each tab on a corner, in path order, moves to
the nearest position along the path (forward first on a tie) that is not
on a corner, whose stretch does not overlap another tab's (as already
placed), and that is at most half the gap to each neighbour away; on an
open path it never passes an end. None: it stays, with `Warning::TabOnCorner
(op, instance, instance_name, profile, distance_mm)`. The search is exact
over the path's corner intervals, not sampled: the allowed set is the
path minus each corner window grown by the half-stretch, and the move is
to its nearest end. A circle of radius `R` turns `D/R` radians per window,
under 30° for `R > 1.91 D`, so the coaster's tabs never move. (changed in
#285: in `dc-cam::tabs::corners`. The half gap is to the neighbours'
positions before any tab moved; the no-overlap rule uses positions as
placed. A moved tab sits 1e-5 mm past the edge it stops at, so its span
does not touch the corner or a neighbour's span. On a closed path whose
length is at most `D`, every position is a corner. It runs inside
`Request::fit` before the #109 rule, on the kept tabs again when that
rule re-spaces or drops some, and so in `op_tabs`, which `tabs place`
uses: a request placed there writes the moved points. `tabs place
--count` sets `avoid_corners` unless `--allow-corners`. A tab moved on an
open path keeps its whole stretch within the path's ends. `tab_on_corner`
carries why the tab stays: `everywhere` when the path is a corner wherever
a tab could go, `no_room` when the neighbours, another tab or an end are
in the way.)

**Start** (`dc-cam::profile`, the seam step that #176 and leads use).
`FirstVertex` and `Near` set each closed outline's seam to the distance
along its tool-centre path of the point nearest the target (the drawn
first vertex, transformed, or the given point; the lowest distance on a
tie). The path is reseamed there before the tab layout, so `at_distances`
count from it and `count` spaces from it; #176's move is skipped (an
entry that does not fit falls back ramp → plunge, warned); a lead is
planned at that one arrival (its radius and sweep ladders still apply)
or dropped with `LeadDropped (no_room)`. `Auto` is the code path of
today. Open paths ignore it. (changed in #285: in
`dc-cam::profile::start`, after `centre_paths` in both `generate_profile`
and `op_tabs`. The nearest point is `nearest_point`'s: first along the
path on a tie, except the middle of an arc centred on the target. The
allowance twin (#284) is re-seamed nearest the same target before
`twin_tabs` carries the tabs onto it, and a reversed last pass keeps its
start. The lead is `lead::plan_lead_in_at_start`. Changed again in #337:
`contour_block` starts a pass's descent at the previous pass Z or the
lap's start Z, whichever is higher, so a lap that starts on a tab never
descends below the tab there, and comes down to its depth past the tab's
end as it leaves any tab; this held for `auto` with a plunge entry too.
The safety pass's S9 refuses a straight plunge below a tab's top within
its span along the bit's path (the outline offset by the radius, and by
the radius plus a last pass's allowance; over a smooth tab's slope, below
the slope the last pass cuts), from the placed tabs and the request's
placement, not the toolpath. Changed in #353: on the path of the passes
before a last pass (the radius plus the allowance) the span is not the
rule, because those passes cut beside the tab, not over it, and
`twin_tabs` ends their raised stretch where their cutter stops touching
the tab the last pass leaves. There S9 refuses a plunge below a tab's top
whose cutter reaches into that material: every point within the radius
of a lap point the span rule holds above the plunge's Z (a rect tab's
whole span, `r + w/2` along the lap with no margin; a smooth tab's hold
and the part of its slope above that Z; the outline's tabs merged) and at
least the radius from every other point of the outline's laps (the
outline offset by the radius both ways), which cut to their floor. The
laps' segments are split exactly at the span ends, and a quadtree over
the cutter, halving cells only where the cutter comes within a cell of
the tab, refuses every cutter that reaches the slack into the tab and
passes every one that, grown by the slack, misses it. The margin of
`TAB_SPAN_MARGIN` still applies to the lap's own plunges; the material
is the span's without it, which is stricter. So the rule is looser than
the span where other stretches of the lap cut the far side of the tab's
kerf (a slot, a corner, the wall opposite: the #353 and #358 cases come
down inside the span, cutting none of the tab on a heightfield), and
stricter on a straight wall, where the
kerf's far side stands the whole span and the cutter, `a` nearer it,
reaches it until about `√(2ar − a²)` past the span.)

Tests: `migrate::v13_to_v14_adds_corner_and_start_defaults`,
`tabs::rotated_square_tabs_leave_corners` (acceptance 47),
`tabs::circle_tabs_never_move`, `tabs::tab_never_moves_past_half_gap`,
`tabs::blocked_tab_warns_tab_on_corner`, `tabs::open_path_tab_stops_at_end`,
`tabs::avoid_corners_false_is_byte_identical`,
`profile::start_near_point_seams_nearest`,
`profile::start_first_vertex_seams_at_vertex`,
`profile::at_distances_count_from_explicit_start`,
`profile::explicit_start_entry_falls_back_not_moves`,
`profile::explicit_start_lead_dropped_when_no_room`,
`profile::open_path_ignores_start`, `cli::corners_and_start_rotated_square`;
for #337, `profile::start_under_a_tab_never_plunges_into_it`,
`safety::s9_refuses_a_descent_into_a_tab` and
`safety::start_under_a_tab_passes_s9`; for #353,
`safety::allowance_pass_leaves_a_tab_at_its_stretch_end`,
`safety::allowance_pass_dropping_onto_a_tab_fails_s9`,
`safety::allowance_pass_past_the_span_onto_a_tab_fails_s9` and the
property cases `inside_last_pass_helix_tabs_pass_first_exports`,
`inside_last_pass_plunge_short_tab_exports` and
`outside_last_pass_small_tab_on_a_hole_exports`.

### Drill holes narrower than the tool (format version 17)

(changed: maintainer decision 2026-10-24.) "Cuts that are too small for
the tool should be skipped, with a warning in the check and export
commands. This is the default, but it can be overridden." Pocket
(`pocket_no_room`) and Inside Profile (`tool_too_large`) already skip
such a shape; Drill drilled it at the tool's size. "Algorithms → Drill",
step 1, now skips it.

`DrillOp` gains `allow_tool_wider_than_hole: bool` after `safe_z_mm`,
written by every save. `false` (the default for a new operation) skips
a hole the tool does not fit with `drill_hole_too_small`; `true` drills
it at the tool's diameter with `drill_tool_wider_than_hole`, as before.
The setting is the project's, not a CLI flag: the CLI writes no Drill
fields, so a user sets it in the `.dcv` as every other Drill field.

**Migration v16 → v17** (`migrate::v16_to_v17`): `allow_tool_wider_than_hole:
false` at the end of each Drill operation; other operations unchanged.
Fails if a Drill already has the key. `false`, not `true`: the decision
is that a hole the bit is too big for is not cut unless the user says
so, so an old project whose Drill was drilling such holes now skips them
and warns. CHANGELOG lists this under "Changed" as a behaviour change.
Version 16 is the tool vendor and product id (#381), which merged first.

Other operations checked: Engrave and On line Profile put the tool
centre on the line by definition, so the groove is the bit's width
whatever the drawing; Left and Right Profiles cut beside an open path
and already skip one whose offset vanishes (`tool_too_large`); Outside
Profile cuts outside the part, and its holes are Inside cuts. None cuts a
drawn shape larger than drawn because the bit is too big, so only Drill
changes.

Tests: `drill::hole_narrower_than_tool_is_skipped` (5 mm, 6.35 mm tool:
no plunge, `DrillHoleTooSmall` with both sizes),
`drill::hole_equal_to_tool_within_tolerance_is_drilled`,
`drill::allow_tool_wider_than_hole_drills_and_warns`,
`drill::every_hole_skipped_warns_no_holes`,
`migrate::v16_drill_migrates_with_allow_tool_wider_than_hole_false`,
`migrate::v16_with_allow_tool_wider_than_hole_already_present_fails`,
`cli::check_reports_drill_hole_too_small`,
`cli::export_reports_drill_hole_too_small_and_writes_no_plunge_there`.

### Bore operation (format version 18)

(changed: maintainer decision 2026-10-24.) "Keep Drill. It's for drill
bits and holes the bit's own size. Add Bore: for each circular target
bigger than the end mill, it spirals down a helix inside the hole at a
set pitch to the depth, then cuts a full circle at the bottom so the
wall is round and to size." Functional capability 14a, acceptance 23a;
the algorithm is "Algorithms → Bore". Drill is unchanged but for its
`drill_hole_too_small` message, which now ends "…; cut it to size with a
Bore and a bit smaller than the hole, use a smaller drill, or set
allow_tool_wider_than_hole to drill it at the tool's size".

**Model.** `Operation::Bore(BoreOp)` (`"kind": "bore"`), between Drill and
Engrave in the enum; the `Operation` accessors cover it. `BoreOp`'s
fields are saved in the order of "Data model". No other type changes.

**Migration v17 → v18** (`migrate::v17_to_v18`): no version 17 file holds
a `"bore"` operation, so the step is the identity and only `version`
changes, as `v9_to_v10` did for the Left and Right sides: the bump is
what ADR-0002 asks for, so a version 17 reader refuses a file with a
Bore with `NewerVersion` instead of a schema error on an unknown kind.
The fixture `crates/dc-project/tests/fixtures/dcv/v17/sample.dcv` loads as
the sample project. If another change takes 18 first, this is the next
free version, rebased on top.

**Validation** (`validate.rs`), as errors: a Bore's `pitch_mm` that is
set and not finite or not more than 0 (`Problem::OutOfRange`, as other
lengths), or more than its tool's step-down
(`Problem::BorePitchOverStepDown { operation, pitch_mm, step_down_mm }`);
a Bore with no `pitch_mm` whose tool has no step-down
(`Problem::ToolValueUnset` on `step_down_mm`, as Pocket's); a negative or
non-finite `allowance_mm` (`OutOfRange`); a Bore whose tool is not a
flat end mill (`Problem::CutterNotFlat`, as a Pocket's): it cuts the wall
with the side of the bit, which a twist drill, V-bit or engraver does not
do to size (Fusion's help names flat and bull-nose end mills for Bore). The CLI writes no
Bore fields; a user adds a Bore in the `.dcv`, as every operation.

**CLI.** `check` and `export` print the new warnings like the others;
`export`'s per-operation summary and the program comments name a Bore
like any operation; `preview` draws it as a cut, not a drill point.

**Plaque.** op3 "Screw holes" becomes a Bore with T2 (`t3`, the 3.175 mm
end mill), `depth_mm` 12, through-cut allowance 0.2, `pitch_mm` null,
`direction` climb, `allowance_mm` 0: the holes come out 5 mm, as drawn.
The operations run op3, op5, op1, op2, op4: one bit change, the holes
before the cut-out, which stays last, op3 before op5 as before. The
programs become `plaque-1-T2.nc` (Bore, Engrave) and `plaque-2-T1.nc`
(Pocket, Pocket, Profile). Every committed plaque program, `.warnings`
file and quick-start excerpt is re-exported in the same PR, each diff
explained in it. The migration corpora's plaque copies are older
versions with a Drill and keep their expected programs.

Tests:
- dc-cam: `bore::five_mm_hole_helix_radius_is_0_9125`,
  `bore::each_turn_falls_at_most_the_pitch`,
  `bore::ends_with_full_circle_at_depth_then_centre`,
  `bore::starts_one_mm_above_stock_top`,
  `bore::climb_is_ccw_conventional_is_cw`,
  `bore::pitch_defaults_to_tool_step_down`,
  `bore::pitch_over_step_down_is_error`, `bore::allowance_shrinks_helix`,
  `bore::descent_feed_is_plunge_limited` (S10),
  `bore::hole_too_small_is_skipped_no_room`,
  `bore::hole_bit_size_is_skipped_pointing_to_drill`,
  `bore::hole_twice_tool_is_bored`,
  `bore::hole_over_twice_tool_is_skipped_leaves_core`,
  `bore::non_circle_is_skipped`, `bore::every_hole_skipped_warns_no_holes`,
  `bore::holes_nearest_first_linked_at_safe_z`,
  `bore::depth_beyond_bottom_needs_allowance` (S1),
  `bore::missing_feed_is_error`,
  `drill::hole_too_small_message_points_to_bore`,
  `order::bore_after_cut_out_warns_cut_inside_freed_part`.
- dc-project: `migrate::version_17_fixture_loads`,
  `migrate::v17_to_v18_changes_only_the_version`,
  `serialize::bore_round_trips`,
  `validate::bore_pitch_over_step_down_is_error`,
  `validate::bore_without_pitch_or_step_down_is_error`,
  `validate::bore_negative_allowance_is_error`,
  `validate::bore_needs_flat_end_mill`.
- dc-gcode: `safety::bore_inside_hole_passes_s9`,
  `safety::bore_doctored_outside_hole_fails_s9` (S9 refuses a program
  whose circle runs 0.1 mm wider), `material::farthest_point_of_an_arc_is_where_it_bulges`
  (an arc is measured where it bulges, not only at its ends and middle),
  and in `tests/bore.rs`: `five_mm_through_hole_program` (acceptance 23a:
  helix radius, pitch, full circle, the swept hole 5 mm ±0.01 through
  the stock), `climb_writes_g3_conventional_g2`, `exports_on_every_post`
  (every bundled definition, mm and inch; the safety pass accepts it),
  `plaque_screw_holes_are_cut_to_their_drawn_size`; the property
  `random_valid_bores_never_panic_and_pass_safety` (as the Pocket and
  Profile properties).
- CLI: `cli::check_reports_bore_no_room_and_leaves_core`,
  `cli::plaque_dxf_exports_same_programs_as_svg` (the new names),
  `quick_start`, and the plaque in the `rs274`, LinuxCNC and GRBL
  simulator corpora (their canned-cycle variants drill the holes as the
  plaque did before).

### Documentation (every item)

Each issue updates, in its PR, the chapters it touches (`profile.md`,
`tabs.md`, `post-processors.md`, `project-file.md` with every new field
and version, `configuration.md`, `warnings.md` for `tab_on_corner`,
`safety.md` for S16), the definitions README where a definition changes,
and CHANGELOG.
