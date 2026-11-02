# Phase 1: Headless MVP — Design Spec

**Status:** accepted
**Functional spec:** `functional.md`

## Overview

Five library crates and one binary. Data flows one way:

```
SVG ──dc-import──▶ FeatureDefinitions ──┐
                                        ├─▶ Project (.dcv, dc-project)
user edits .dcv ────────────────────────┘        │
                                                 ▼
                       dc-cam::profile::generate(project, op) ─▶ Toolpath (project coords, mm, Y-up)
                                                 │
                                                 ▼
                       dc-gcode::emit_programs(project, toolpaths) ─▶ Programs (text) + warnings
                                                 │
                                                 ▼
                                   safety checks on the parsed Program
```

`dc-geometry` underlies all of it. The CLI is a thin shell over these calls
and owns no logic.

Everything in the project and the toolpath is millimetres, Y-up, Z-up, with
stock bottom at Z = 0 and stock top at Z = thickness. Display units and the
machine origin are applied only in the CLI's `init` defaults and in
`dc-gcode::emit`.

## PureCutCNC sources

| DreamCarve | PureCutCNC | Port notes |
| --- | --- | --- |
| `dc-geometry::polyline` | `src/geometry/`, `src/store/helpers/clipping.ts` (profile↔path), `src/engine/toolpaths/geometry.ts` | Clipper integer scaling disappears; cavalier_contours works in f64 with bulge arcs. PureCutCNC's fixed-density `DEFAULT_FLATTEN_*` constants are not ported: they exist to map Clipper output back to source arcs, and DreamCarve flattens by `flatten_tolerance_mm` instead. |
| `dc-project::model` | `src/types/project.ts` (`Project`, `Stock`, `MachineOrigin`, `Tool`, `FeatureDefinition`, `FeatureInstance`, `Tab`, `Operation`) | Only the fields phase 1 needs. `z_top`/`z_bottom` kept on instances. Tool feeds become `Option` so "unset" is representable. |
| `dc-project::history` | `src/store/projectStore.ts` undo/redo | Snapshot list, no UI. |
| `dc-cam::profile` | `src/engine/toolpaths/edge.ts` (`generateEdgeRouteToolpath`, `edgeRouteTargetWalls`), `multiFeature.ts` for nesting | Only the plain contour path: no trochoidal, no keep-out holes, no entry policy. Winding re-derived for Y-up. |
| `dc-cam::tabs` | `src/engine/toolpaths/tabs.ts` (`applyTabsToEdgeRoute`, `toolCentreContours`, `tabLayoutFreeFraction`, `applyTabWarnings`), `tabs.test.ts` | Tabs are specified per operation by count or explicit positions along the path; PureCutCNC's world-placed `Tab` rectangles arrive in phase 5 with the GUI. Smooth tabs are not PureCutCNC's raised cosine (`tabProfile.ts`), which reaches the tab top at one point and leaves about 60% of the height under the bit; they leave a triangular bridge instead (#66). |
| `dc-cam::passes` | depth stepping inside `edge.ts` | Final pass clamps to depth, never deeper. |
| `dc-project::post_processor` | `src/engine/gcode/types.ts` (`MachineDefinitionSchema`) | Keys snake_case. Every field required (upstream zod defaults dropped) so a definition file shows every value. Lives in `dc-project`, with the bundled definitions, because loading a project needs them (#106). |
| `dc-gcode::post` | `postprocessor.ts` (`runPostProcessor`), `utils.ts` (`projectToMachinePoint`, number formatting) | `projectToMachinePoint` loses its Y mirror; axis mapping and inversion kept. Canned cycles and coolant parsed but unused until phase 2. |
| `dc-project/definitions/<id>/v<n>.json` | `src/engine/gcode/definitions/grblhal.json`, `grbl.json` | Copied as data, keys converted to snake_case, the five project settings moved into `default_settings` (#106). Attribution in `NOTICE` and `definitions/README.md`; the generator name in the header becomes DreamCarve's (`{date}` stays a placeholder filled at export). |
| `dc-import::svg` | `src/import/svg.ts`, `normalize.ts`, `svg.test.ts` | Parsing delegated to usvg, which already resolves transforms, `use`, and shapes to paths. The flattening, Y flip, and definition grouping are ours. |
| `tests/golden/profile-*` | captured via `tools/parity/` | See Test plan. |

Files in the first column that port logic (not just data) carry the upstream
attribution header.

## Data model

### dc-geometry

```rust
/// A 2D point in millimetres, Y-up.
pub struct Point { pub x: f64, pub y: f64 }

/// A polyline vertex. `bulge` is tan(θ/4) of the arc to the next vertex;
/// 0 means a straight segment. Same convention as cavalier_contours and DXF.
pub struct Vertex { pub x: f64, pub y: f64, pub bulge: f64 }

/// Open or closed sequence of line and arc segments.
pub struct Polyline { vertices: Vec<Vertex>, closed: bool }

/// A closed polyline with known orientation. Outer boundaries are CCW,
/// holes are CW (ADR-0001). Constructors normalise, and reject an outline
/// that crosses itself (SelfIntersecting, with the first crossing point).
pub struct Contour(Polyline);

/// One or more contours forming a region: one outer, zero or more holes.
pub struct Region { outer: Contour, holes: Vec<Contour> }

/// 2D affine transform (wraps kurbo::Affine).
pub struct Transform(kurbo::Affine);

pub fn offset(c: &Contour, delta_mm: f64) -> Vec<Contour>;       // cavalier_contours parallel_offset; +delta grows
    // #465: an offset cavalier_contours returns as open pieces (a traced convex curve offset
    // inward by about its radius) is taken again with position and distance tolerances of
    // 1e-6, 1e-7, then 1e-8 mm (its defaults are 1e-5 and 1e-4), and the first that closes
    // every piece is kept
pub fn flatten(path: &kurbo::BezPath, tolerance_mm: f64) -> Vec<Polyline>;
    // within tolerance_mm of the curve: kurbo is asked for half of it, as the SVG importer's
    // per-curve flattening is (changed in #225: kurbo's own bound is approximate, and at the
    // tolerance itself it strayed up to 1.34 × it from random cubics)
pub fn nest(contours: Vec<Contour>) -> Result<(Vec<Region>, Vec<GeometryWarning>), GeometryError>;
    // even-odd containment → outer/holes; crossing shapes are ContoursCross, duplicates dropped with a warning
pub fn length(p: &Polyline) -> f64;
pub fn point_at(p: &Polyline, distance: f64) -> (Point, Vec2);    // position and tangent
pub fn bounds(p: &Polyline) -> Rect;
pub fn distance_mm(a: &Polyline, b: &Polyline) -> f64;          // shortest gap between lines, arcs exact; 0 when they touch
```

Errors: `GeometryError::{Degenerate, SelfIntersecting, ArcUnderNonUniformTransform, ContoursCross, CoordinateOutOfRange}`.
An offset that consumes a contour is not an error: `offset` returns no
contours, and the generator decides what that means (#71).
Warnings: `GeometryWarning::DuplicateContourDropped`. The nesting rules, with
figures, are in the user guide chapter "Shapes, holes, and islands".

### dc-project

```rust
pub const CURRENT_VERSION: u32 = 6;

pub struct Project {
    pub version: u32,
    pub name: String,
    pub display_units: Units,                 // Mm | Inch; affects CLI and init only
    pub flatten_tolerance_mm: f64,            // default 0.01
    pub stock: Stock,
    pub origin: MachineOrigin,
    pub tools: Vec<Tool>,
    pub operations: Vec<Operation>,
    pub instances: Vec<FeatureInstance>,
    pub post_processor: PostProcessorRef,     // bundled definition id + version, and settings (#106)
    pub definitions: BTreeMap<DefinitionId, FeatureDefinition>,  // last: the bulk of the file
}

pub struct Stock { pub width_mm: f64, pub height_mm: f64, pub thickness_mm: Option<f64> }
// thickness_mm is None until the user sets it (#64); validation errors, nothing defaults it.
// Stock occupies [0,width]×[0,height]×[0,thickness] in project space.

pub struct MachineOrigin {
    pub xy: OriginXy,     // BottomLeft | BottomRight | TopLeft | TopRight | Centre | Custom{x,y}
    pub z: OriginZ,       // StockTop | StockBottom
}

pub struct FeatureDefinition { pub id: DefinitionId, pub name: String, pub profiles: Vec<Polyline> }

pub struct FeatureInstance {
    pub id: InstanceId, pub definition: DefinitionId, pub transform: Transform,
    pub name: String, pub visible: bool, pub locked: bool,
    pub z_top_mm: Option<f64>, pub z_bottom_mm: Option<f64>,
}
// z_top_mm / z_bottom_mm are None until the user sets them (#99); phase 1
// reads neither, and validation range-checks them only when set.

pub struct Tool {
    pub id: ToolId, pub name: String, pub cutter: ToolKind, // EndMill | BallNose | VBit{included_angle_deg} | Engraver
    pub diameter_mm: f64, pub flutes: u8,
    pub feed_mm_min: Option<f64>, pub plunge_mm_min: Option<f64>, pub rpm: Option<u32>,
    pub step_down_mm: Option<f64>, pub step_over_fraction: Option<f64>,
}

pub enum Operation { Profile(ProfileOp) }

pub struct ProfileOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>, pub tool: ToolId,
    pub side: Side,                         // Inside | Outside | OnLine
    pub depth_mm: Option<f64>,              // from stock top; None until set (#64), a validation error
    pub step_down_mm: Option<f64>,          // overrides tool.step_down_mm
    pub direction: CutDirection,            // Climb | Conventional
    pub through_cut_allowance_mm: f64,      // default 0; explicit permission to go below stock bottom
    pub safe_z_mm: f64,                     // above stock top
    pub tabs: Option<TabSpec>,
}

pub struct TabSpec {
    pub placement: TabPlacement,            // Count(u8) | AtDistances(Vec<f64>)
    pub width_mm: f64, pub height_mm: f64,  // height above stock bottom
    pub shape: TabShape,                    // Rect | Smooth
}

pub struct History { snapshots: Vec<Project>, cursor: usize }  // push/undo/redo, no UI
```

Serialization: serde + serde_json, pretty-printed, keys in declaration order.
`Project` declares its fields in the order a person edits them (#76):
settings first (`version`, `name`, `display_units`, `flatten_tolerance_mm`,
`stock`, `origin`, `tools`, `operations`), then `instances`,
`post_processor`, and the geometry, `definitions`, last. `save` writes
through a custom `serde_json::ser::Formatter` (`layout.rs`) that puts each
vertex object of a `vertices` array on one line,
`{ "x": 137.684, "y": 88.535, "bulge": 0.0 }`, and is otherwise serde_json's
pretty printer. Neither change touches the schema: the keys, types and
number digits are the same, so there is no version bump, readers accept any
key order and layout, and output stays byte-for-byte deterministic.
`OriginXy` and `ToolKind` save a choice without data as a plain string
(`"xy": "bottom_left"`) and one with data as a one-key object
(`"xy": { "custom": { "x_mm": 20.0, "y_mm": 5.0 } }`); version 3 made this
change (#46). Version 4 made `stock.thickness_mm` and each Profile's
`depth_mm` nullable, so `init` can leave them unset (#64); the migration
carries every version 3 number over unchanged. Version 5 added
`post_processor.tool_change.mode` (`ToolChangeMode::{SeparatePrograms,
M6}`, `"separate_programs"` | `"m6"`); the migration sets
`"separate_programs"` (#95). Version 6 made each instance's `z_top_mm`
and `z_bottom_mm` nullable, so `init` and the SVG importer leave them unset
(#99); the migration carries every version 5 number over unchanged.
Version 7 replaced the embedded post-processor by a reference (#106,
ADR-0004; see "Post-processor reference" below). Validation reports
`tool_change_mode` `"m6"` on a definition whose `tool_change.commands` has
no `M6` line as `Problem::ToolChangeModeNeedsM6`, located at the field.
`load(bytes) -> Result<Project, LoadError>` reads `version`, applies
migrations in sequence up to `CURRENT_VERSION`, then validates with
`validate(&Project) -> Vec<Problem>` (dangling ids, non-positive dimensions,
magnitudes beyond `dc_geometry::MAX_COORDINATE_MM`, an unsafe post-processor
`file_extension`, and every value the user must supply that is unset: the
stock thickness, an operation's depth, and, for a tool an enabled operation
uses, its feed and plunge rate and its RPM under automatic spindle control
(#64, #76), and its step-down when an enabled operation using it sets none
(#98; `Problem::ToolValueUnset` at the tool, once per tool; dc-cam's
`CamError::MissingStepDown` stays as the library's own guard); disabled operations, open profiles, and tools no enabled
operation uses (`Problem::UnusedTool`, located at the tool, with its unset
rates not reported) as warnings). Problems come in file order. Tool problems name the
tool by name and id. A file with a newer version
fails with `LoadError::NewerVersion`.

Sanity checks (#67, functional spec S8) run in the same pass and look
only at enabled operations and the tools they use. Errors: a feed or
plunge rate above `MAX_RATE_MM_MIN` (20 000 mm/min, `Problem::ToolOutOfRange`),
`Problem::CutterNotFlat` (an Outside or Inside Profile whose tool is not
`ToolKind::EndMill`; `ToolKind::is_flat`; `Side::OnLine` accepts any
cutter, since the tool centre follows the line), and
`Problem::OperationHasNoTargets`. A `line_number_increment` of 0 was an
error here until #106; a project can no longer change it, so
`post::bundled_line_number_increments_are_positive` checks every bundled
version instead. Warnings: `Problem::StepDownAboveDiameter`
(located at the step-down in effect, the operation's or else the tool's),
`Problem::PlungeAboveFeed`, `Problem::SpindleSpeedHigh` (above `MAX_RPM`,
60 000) and `Problem::LargeDiameter` (above `MAX_TOOL_DIAMETER_MM`, 50 mm).
The thresholds are public constants in `validate.rs`. `generate_profile`
also refuses a cutter that is not flat on Outside or Inside
(`CamError::CutterNotFlat`), so a
caller that skips validation never gets a wrong-size part.

Locations (#76): `Problem::field()` names the value a problem is about as
the messages write it (`stock.thickness_mm`, `tools[t1].feed_mm_min`;
a bracket after an array picks the element by `id`, else by index).
`SourceMap::parse(&str)` (`locate.rs`) scans the JSON text itself, since
serde_json keeps no spans, and records the 1-based line and column
(in characters) of every value; `locate(field)` and
`position(&[PathSegment])` look one up. It reads the file as the user has
it, so positions stay right after hand reformatting or reordering. The CLI
turns them into `coaster.dcv:9:21:` prefixes. Schema errors (#65) reuse
`position`: `load` deserializes through `serde_path_to_error`, and
`LoadError::Schema` carries the path (`operations[0].tabs.placement`) and,
when the file as written has a value there, its line and column. One lookup
serves current and migrated files. serde reads the internally tagged
`Operation` whole, which loses the path inside it, so `load` reads a
failing operation's fields again as its variant's struct to recover it.
Every `Option` field is required on load (`null` when unset), and so is
every field of a post-processor definition file.

#### Post-processor reference (#106, ADR-0004)

A project stores which bundled definition version it uses and the user's
settings, not the definition:

```rust
pub struct PostProcessorRef {               // `post_processor` in a .dcv, keys in this order
    pub id: String,                         // "grblhal"
    pub definition_version: u32,            // definitions/<id>/v<n>.json
    pub file_extension: String,
    pub spindle_control: SpindleControl,
    pub spindle_on_dwell_s: f64,
    pub tool_change_mode: ToolChangeMode,
    pub decimal_places: DecimalPlaces,
}
impl PostProcessorRef {
    pub fn latest(id: &str) -> Result<Self, PostProcessorError>;          // init, export --post
    pub fn definition(&self) -> Result<&'static BundledDefinition, PostProcessorError>;
    pub fn resolve(&self) -> Result<PostProcessor, PostProcessorError>;   // what dc-gcode emits with
}
pub fn bundled_definitions() -> Result<&'static [BundledDefinition], PostProcessorError>;
pub fn latest_definitions() -> Result<Vec<&'static BundledDefinition>, PostProcessorError>;
pub fn bundled_definition(id: &str, version: u32) -> Result<&'static BundledDefinition, PostProcessorError>;
pub fn latest_definition(id: &str) -> Result<&'static BundledDefinition, PostProcessorError>;
```

`resolve` takes the definition's `PostProcessor` and puts the five settings
where the full schema has them (`file_extension`,
`feed_speed.spindle_control`, `feed_speed.spindle_on_dwell.duration_s`,
`tool_change.mode`, `number_format.decimal_places`), so `dc-gcode` emits
from the same `PostProcessor` as before and emission is unchanged.

**Definition files.** `crates/dc-project/src/definitions/<id>/v<n>.json`,
compiled in with `include_str!` and parsed on first use. A file is a
`PostProcessor` without the five settings, plus a `default_settings` object
holding them under the project's names: the settings `init` and `--post`
write. Defaults are per definition because they are controller facts (the
3 s spin-up dwell is cited in the definitions README; a future LinuxCNC
definition would default to `ngc`). Parsing fails if a file also sets one
of the five itself. Version 1 of `grblhal` and `grbl` is today's file with
the settings moved out; `resolve_matches_old_embedded_definition` checks
that each resolves, with its defaults, to exactly what the old file parsed
to, kept in `tests/fixtures/post/`.

**Versions.** A change to a definition that changes emitted bytes is a new
version file; released versions stay in the binary unchanged.
`released_definition_versions_are_unchanged` pins every file by an FNV-1a
64-bit hash (no dependency; it detects edits and is not a security
measure), and `.gitattributes` keeps them LF so the hash holds on Windows.
`export --post <id>` writes the latest version with its default settings,
as switching always reset them; on the project's own id it is how a user
upgrades, and export prints `post-processor grblhal v1 → v2`.

**Load.** After deserializing, `load` resolves the reference: an unknown
`id` is `LoadError::Schema` at `post_processor.id`, an unknown
`definition_version` at `post_processor.definition_version`, each with the
file position and a list of what is bundled.

**Migration to version 7** (`v6_to_v7`). The five settings move out of the
embedded copy with their values. The rest of the copy is compared, as JSON,
with the bundled version 1 of its `id`. Equal: the step writes the
reference. Different, or an `id` that is not bundled: the step fails, and
the message names every differing field path with both values, then the
reference block (the user's settings filled in) to paste in place of the old
one, with `"version"` set to 7. For an unbundled id the comparison is with
the bundled version 1 that differs least. A different dialect is never
swapped in silently, since that would change the G-code.

**Where the registry lives.** In `dc-project`, not `dc-gcode`: `load` must
reject an unknown id or version, validation needs the definition's tool
change (`"m6"` needs an `M6` line, #95), and the version 6 migration compares
with version 1, all inside `dc-project`, which cannot depend on `dc-gcode`.
Keeping it in `dc-gcode` with validation taking the resolved definition as a
parameter would still leave `load` and the migration without it.
`dc-gcode::post` keeps the tests of the controller facts, run over every
version of every definition, because some of them emit G-code.

**`line_numbers` is not a setting.** The chapter never listed it among the
settings a user may change, both bundled definitions have it off, and
whether a controller accepts `N` words, and in what range, is controller
data that belongs with the verified definition. So `line_numbers` and
`line_number_increment` stay in the definition, and the #67 check that the
increment is above 0 becomes a test on every bundled version. For the same
reason the check that the dwell command has `{seconds}`
(`Problem::MissingPlaceholder`, removed) is now part of
`post::bundled_definitions_dwell_three_seconds_after_spindle_on`.

### dc-cam

```rust
pub struct Toolpath { pub moves: Vec<Move> }
pub enum MoveKind { Rapid, Linear, ArcCw { centre: Point }, ArcCcw { centre: Point } }
pub struct Move { pub kind: MoveKind, pub to: Point3, pub feed_mm_min: Option<f64> }
// Rapids carry no feed. Every Linear/Arc carries Some(feed).

// #248: one variant per WarningCode, with typed fields; `code()` gives the
// stable snake_case code and `Display` the message.
pub enum Warning { OpenPathSkipped { shape: ShapeRef, reason: OpenPathReason }, /* … */ }

pub fn generate_profile(project: &Project, op: &ProfileOp) -> Result<(Toolpath, Vec<Warning>), CamError>;

// #62: the cutter's XY reach, and how far it runs past each stock edge.
pub struct Overhang { pub min_x_mm: f64, pub max_x_mm: f64, pub min_y_mm: f64, pub max_y_mm: f64 }
pub fn swept_bounds(toolpath: &Toolpath, tool_radius_mm: f64) -> Option<Rect>;
pub fn stock_overhang(stock: &Stock, swept: Rect) -> Option<Overhang>;
```

### dc-gcode

`PostProcessor` is a serde struct mirroring PureCutCNC's `MachineDefinition`
schema field for field (id, name, description, vendor, builtin,
file_extension, coordinate_system, number_format, units, program,
work_coordinates, motion, feed_speed, tool_change, canned_cycles, coolant,
stop). It lives in `dc-project::post_processor`, with the bundled
definitions (see "Post-processor reference" above), because the crate
direction (`dc-geometry → dc-project → dc-gcode`) forbids `dc-project`
depending on `dc-gcode`; `dc-gcode` re-exports it. `emit` and
`emit_programs` resolve the project's `PostProcessorRef` and emit with the
result; one that names no bundled definition is `ExportError::PostProcessor`.

```rust
pub struct Program { pub text: String, pub extension: String, pub pauses: Vec<Pause>, pub first_tool: FirstTool, pub extents: Extents }
pub struct Extents { pub units: Units, pub x: Option<AxisRange>, pub y: Option<AxisRange>, pub z: Option<AxisRange> }  // machine coordinates, program units
pub struct AxisRange { pub min: f64, pub max: f64 }
pub struct Pause { pub line: usize, pub message: String }
pub struct FirstTool { pub number: usize, pub name: String }
pub fn emit(project: &Project, ops: &[(ProfileOp, Tool, Toolpath)]) -> Result<(Program, Vec<Warning>), ExportError>;
pub fn emit_programs(project: &Project, ops: &[(ProfileOp, Tool, Toolpath)]) -> Result<(Vec<Program>, Vec<Warning>), ExportError>;
pub fn parse_motion(text: &str) -> Vec<MachineMove>;   // test-only helper, re-exported for safety tests
```

`emit` writes one program. `emit_programs` is what `export` calls: with
the tool-change mode `"separate_programs"` it splits the enabled operations
into tool runs (consecutive operations with the same tool, in operation
order; never regrouped) and returns `emit` of each run, so each program
passed the safety pass on its own and has its own extents; with `"m6"` it
returns `emit` of all of them. Any error fails the whole call (#95).

`ExportError::{MissingFeed{tool, field}, MissingRpm{tool}, BelowMaterialBottom{op, z}, RapidBelowSafeZ, NoEnabledOperations, UnknownPostProcessor{id}, BundledDefinition{file, source}}`.
`BundledDefinition` means a bundled JSON file failed to parse, which only a
broken build can cause; it keeps `dc-gcode` free of panics.

## Algorithms

### Profile contour (dc-cam::profile)

Inputs: project, op. Output: toolpath in project space.

1. Resolve each target instance: apply `transform` to every profile of its
   definition. Closed polylines become `Contour`s (normalised CCW). Open
   polylines raise `Warning::OpenPathSkipped` and are dropped. A closed
   polyline that encloses no area or crosses itself fails the operation
   with `CamError::InvalidTarget` naming the instance (for a crossing,
   `GeometryError::SelfIntersecting` with the point where it crosses).
2. `nest` the contours of all targets into `Region`s. Outer contours are
   CCW, holes CW. Crossing shapes fail the operation with
   `CamError::Geometry(GeometryError::ContoursCross)`, naming both
   instances; a dropped duplicate becomes `Warning::DuplicateShapeDropped`.
3. Tool-centre contours. Let r = diameter / 2.
   - `Outside`: offset outer by +r; offset each hole by −r (the cutter runs
     inside the hole, since the hole's inside is "outside the part").
   - `Inside`: offset outer by −r; offset each hole by +r.
   - `OnLine`: no offset.
   Offsets that vanish (hole smaller than the tool) raise
   `Warning::ToolTooLarge` and that contour is skipped.
   Overlap check (`dc-cam::overlap`, decision D6 in
   `docs/research/toolpath-ordering.md`): for every pair of tool-centre
   paths whose shapes lie in different regions, if either path comes closer
   than r to the other shape's line, its swept tool enters material that
   shape keeps; raise one `Warning::ToolpathsOverlap` per pair of shapes,
   in cut order, naming both and the smallest gap between their lines.
   A part and its own holes are one region and never compared. Close
   outlines are not merged (PureCutCNC's `edgeLevelRuns` does that).
4. Direction. Spindle is clockwise viewed from above. Climb milling on an
   outside wall traverses the tool-centre contour clockwise; on an inside
   wall, counter-clockwise. Conventional is the reverse. A contour that is
   an "outside wall" is the outer's +r offset and each hole's +r offset
   (Inside mode); an "inside wall" is every −r offset and OnLine contours
   of holes. Reverse the polyline as needed. This rule is a unit test, not a
   comment.
5. Depth passes. `step = op.step_down_mm.or(tool.step_down_mm)` else
   `CamError::MissingStepDown`. Pass depths d₁..dₙ from the stock top:
   `d_k = min(k·step, depth)`, stopping when `d_k == depth`. Z of pass k is
   `thickness − d_k`. If `depth > thickness + through_cut_allowance`, return
   `CamError::BelowMaterialBottom`.
6. Emission per contour, per pass: rapid to (start.xy, safe Z); linear
   descent to pass Z at plunge feed; traverse the contour at feed, ending at
   the start vertex; after the final pass of a contour, rapid to safe Z.
   Arc segments (bulge ≠ 0) emit as `ArcCw`/`ArcCcw` with their centre;
   `dc-gcode` decides whether to keep or flatten them.
7. Tabs (`dc-cam::tabs`), applied to each pass whose Z is below the tab top
   `tab.height_mm`: for each tab centre distance along the tool-centre
   contour (`Count(n)` spaces them at `L/n` intervals starting at `L/(2n)`;
   `AtDistances` uses the given values modulo L), the raised span is
   `width + diameter` centred on it, since the cutter's full width must clear
   the tab (ported from `toolCentreContours`). Rect tabs: split the path at
   span boundaries, lift to tab Z with a vertical linear move at plunge
   feed, traverse at feed, descend at plunge feed. Smooth tabs leave a
   triangular bridge with base `width` and peak `height_mm` (#66): the
   tool centre holds the tab top for one bit diameter centred on the tab
   and ramps straight down to the pass Z over `width / 2` on each side, so
   the span is still `width + diameter`. Every move is at feed; a ramp on
   an arc is split into chords within `flatten_tolerance_mm` in XY. Tab
   spans that overlap merge; a merged smooth span ramps over `width / 2`
   at each end and holds the top between. Warnings: `TabsOverlap`, `TabsOutsideCutRange`
   (ported from `applyTabWarnings`), and `TabsCoverMostOfContour` when the
   merged spans cover more than half of a contour's length, so the part or
   hole may not cut free. That warning names the shape, path length, tab
   width, tool diameter, path each tab raises, tab total and share, and suggests a count or width that fits; the tabs are
   still cut as set (maintainer decision on #63).
8. Stock bounds (#62, `dc-cam::stock`). The swept extent is the XY
   bounds of the toolpath's cutting moves (rapids are at safe Z, above the
   stock), each arc counted to its axis extremes, widened by r. If it
   runs past the stock rectangle `[0, width] × [0, height]` by more than
   `EPS_MM` on any side, return `CamError::OffStock { op, overhang }`,
   whose message names the operation and each side's overhang in mm
   ("operation `op1` cuts off the stock: 71.350 mm past the +X (right)
   edge"). It is an error, like a depth below the floor: the cutter would
   leave the material and may hit clamps. The check lives in `dc-cam`
   because it needs the tool radius and the stock and is a property of the
   toolpath, not of the G-code; `dc-gcode` repeats it on the parsed program
   (S7) because the post-processor does not trust its callers.
9. Return moves and warnings.

### Emission (dc-gcode::emit)

1. Preconditions: every referenced tool has `feed`, `plunge`, `rpm`, else
   `ExportError::MissingFeed`/`MissingRpm`. With
   `spindle_control: manual`, `rpm` is optional. At least one
   enabled op.
2. Compute machine transform from `origin`: translation `(−ox, −oy, −oz)`
   with `oz = thickness` for `StockTop`, 0 for `StockBottom`; then axis
   mapping and sign from `coordinate_system`. No mirror of Y.
3. Units: if `post.units` wants inches (determined by the project's
   `display_units`, which `init` and `--post` keep aligned), scale by 1/25.4
   after the transform, and emit `inchCommand`; otherwise `mmCommand`.
4. Header lines with placeholders `{programName}`, `{date}`, `{unitsCommand}`,
   `{wcsCommand}`, then `G90`, the units command and
   `workCoordinates.selectCommand`, each only if not in the header. The
   header's lines that only set modal state (`G17`, `G20`/`G21`, `G54`–`G59`,
   `G90`, `G94`), plus any of those three added, form the setup that is
   written again after every pause that more motion follows (#55). If the
   enabled operations change tools, and the post-processor's
   tool-change mode is not `"m6"` or its tool change has no `M6` line,
   `emit` fails with `ToolChangeNotSupported`, naming the tools
   (`emit_programs` never asks it to in `"separate_programs"` mode, and
   validation refuses `"m6"` without `M6`); otherwise `M61 Q{n}` for the
   first tool follows the setup,
   because grblHAL skips an `M6` for the tool it holds and keeps that
   number from the previous job (#57).
5. Per operation: `operationHeader` with `{operationIndex}`,
   `{operationName}`, `{toolNumber}`, `{toolName}`. On the first op, a
   retract to safe Z (Z is unknown at program start) and no stop: the user
   fitted the first tool and set work zero with it before starting (#57).
   On a tool change, the tool change block: a retract to safe Z, then
   `spindle_off` if the spindle runs, then the definition's commands. Its
   `M6` line becomes DreamCarve's own `T{n} M6 (MSG, Fit T{n} {name}. Set Z
   zero, then resume.)`, with the name shortened to keep the message within
   96 characters; lines that only pause (`M0`, `M1`) and
   `pause_after_change` are left out, and so are lines that only stop a
   spindle that is already off. After the block the setup is written
   again, the position is unknown (the user may jog, probe or set Z zero),
   so the program retracts to safe Z again, and the next move writes its
   motion and feed words in full (#43). Then spindle
   on with `S{rpm}`; then
   `feed_speed.spindle_on_dwell.command` with `{seconds}` filled, unless its
   `duration_s` is 0 (after every spindle start, including after a tool
   change); then moves. After the last op of a tool, `spindle_off` before a
   tool change, and at the end unless the footer has it.
   With manual spindle control: no spindle words and no dwell. Lines of the
   tool-change block and footer that only stop the spindle are left out.
   After the first op's retract, or the tool change and its second
   retract, before the first move of each tool, pause for the router:
   `M0 (MSG, Turn the router ON. Speed {rpm} RPM. Resume to start cutting.)`,
   leaving out the speed when the tool has none, then the setup again and,
   as after any stop, a retract to safe Z (#68). After the last op of a tool
   (before a tool change, and at the end), retract to safe Z, then
   `M0 (MSG, Turn the router OFF.)`. The message is always a `( )` comment,
   whatever the dialect's comment syntax, passes the comment sanitizer, and
   the pause is always `M0`, never `pause_command`. Rapids use
   `rapidCommand`, cuts `linearCommand` with `F` on change (or always, if
   `!modalFeedSpeed`). Arcs: if `motion.arcInterpolation`, emit
   `cwArcCommand`/`ccwArcCommand` with IJ (or R) per `arcFormat`; otherwise
   flatten at `flatten_tolerance_mm`. Numbers formatted per `numberFormat`.
   Modal motion words omitted when `modalMotion`.
6. Footer, `programEndCommand`.
7. Safety pass over the emitted text using `parse_motion`: S1 (Z ≥ machine
   bottom − allowance), S2 (rapids end at or above machine safe Z, and an
   XY rapid starts at a known Z at or above it), S3 (every
   G1/G2/G3 has an active F), S4 (spindle running and dwelled before a cut,
   or with manual control an `M0` or `M1` after the last cut and the last
   `M6` before each tool's first cut, and no `M3`, `M5` or `S` word; in
   both modes every `M0`, `M1` or `M6` at a known Z at or above safe Z).
   Z is one state for S2 and S4: unknown at program start and after every
   `M0`, `M1` or `M6` until a line writes Z, whatever Z the program wrote
   before the stop. A rapid in XY, a stop, and an `M3` all need a known Z
   at or above safe Z, so a retract follows every stop before any of them
   (#43, #68). S5 (units, WCS and `G90` before
   the first move or spindle start, and again after every stop), S6 (each
   operation's motion lines, mapped back through the machine transform,
   are its toolpath move for move: rapids split only into their Z and XY
   parts, arcs as `G2`/`G3` with the toolpath's direction and centre, or
   as straight moves on the arc in order within the flatten tolerance, and
   nothing else but retracts before the first move and after the last; a
   move that ends where the controller already is, within the rounding,
   may be written as nothing, a straight move or an arc of at most a half
   turn (changed in the property-test fixes of #225–#235: an offset ring
   began 0.0017 mm before the end of a round corner, and the arc piece
   left, shorter than a unit of a 4-place inch program, was written as
   nothing and failed S6),
   S7 (every cutting move, mapped back to project space with its arc
   extremes and widened by its operation's tool radius, stays in the stock
   rectangle within one unit in the last written place:
   `ExportError::OffStock { op, line, side, overhang_mm }`, #62). An arc
   anywhere in the program whose end is its start within half a unit in
   the last written place is a full circle to the controller and fails
   as `ExportError::FullCircleArc`, so S7 and the extents never see one.
   `M6` and `M61` are accepted only from a definition whose tool change has
   `M6` and whose tool-change mode is `"m6"`. A violation is
   `ExportError`, never a warning.
8. `Program.pauses` lists every `M0` and `M6` line with its 1-based line
   number and its comment text, so the CLI can tell the user what each stop
   is for when the sender does not show the message. `Program.first_tool`
   is the tool the user fits and zeroes with before starting; the CLI
   prints "Before you start: fit T{n} {name} and set work zero with it."
   `Program.extents` is the range of every machine axis the parsed program
   reaches, in its own units: every known end point of every move, and the
   axis extremes of every arc (from I/J, or the centre GRBL computes from
   R), so it can be wider than the X and Y words (#62).

### SVG import (dc-import::svg)

1. Parse with `usvg::Tree::from_data` with default options. Determine
   mm-per-user-unit: if the root `width`/`height` carry `mm`, `cm`, or `in`
   units, use them; otherwise assume 96 px per inch (CSS). `init` reports
   which was used.
2. Walk the tree. Each `usvg::Path` becomes one `FeatureDefinition` named
   from its `id` or `path-N`. Its `tiny_skia_path` is converted to a
   `kurbo::BezPath`, transformed by the node's absolute transform, scaled to
   mm, and flipped: `y' = height_mm − y`.
3. `flatten` at `flatten_tolerance_mm`. Closed subpaths become closed
   polylines; open stay open. Zero-area closed polylines are dropped with a
   warning. (Changed in phase 2, #70: the cubics usvg makes of circular
   arcs are recognised exactly and kept as bulges, and only other curves
   are flattened; see `crates/dc-import/src/svg/arc.rs`.)
4. Each definition gets one instance with identity transform and
   `z_top_mm`, `z_bottom_mm` unset (`None`, #99).

Text, images, gradients, and clip paths are ignored with a warning. usvg
must be ≥ 0.45 (Apache/MIT; earlier versions are MPL-2.0).

**Coordinate limit (#59).** `dc_geometry::MAX_COORDINATE_MM` is 6000 mm
(6 m), larger than any hobby CNC table. It bounds numeric range and catches
typos; the machine envelope is the controller's soft limits. One constant
serves every check, so a drawing that imports never fails validation on
size:

- The importer refuses a page side or any path control point beyond it.
- `validate` reports every vertex coordinate, transform coefficient, stock
  side, profile reach (arc extremes included) and transformed instance
  reach beyond it, naming the field (`instances[i1].transform`).
- `Transform::apply_polyline` returns `GeometryError::CoordinateOutOfRange`
  when a result is non-finite or beyond it, so `dc-geometry` is safe on its
  own: `cavalier_contours` panics on overflowed input.
- `init` refuses a drawing whose stock, with its margin, would exceed it.

### CLI (dreamcarve)

```
dreamcarve init <svg> [-o <dcv>] [--post grblhal|grbl] [--units mm|inch]
dreamcarve export <dcv> [-o <nc>] [--post <id>]
dreamcarve check <dcv>
dreamcarve posts
```

`init` builds a project: stock = drawing bounds plus 10 mm margin each side,
thickness `None` (the instances' `z_top_mm` and `z_bottom_mm` are `None`
too, #99; phase 1 never reads them). The instance offset and the stock size are rounded up to
0.01 mm, ignoring up to 0.001 mm of flattening noise above a multiple, so
the margin holds to the program's 0.001 mm resolution and the file reads
cleanly. Origin BottomLeft/StockTop, one tool "Set me" with
`diameter 6.35`, flutes 2, feeds `None`, one Profile op targeting every
closed instance, outside, depth `None`, step-down `None` (the tool's,
also `None`, applies; #98), climb, safe Z 5, tabs
`Count(4)` 8×3 rect. `init` then lists every unset value the user must
supply, one per line as `coaster.dcv:line:column: <field> is not set (<unit>)`,
pointing at its `null`. Exit codes: 0 ok, 1 validation/export error, 2 usage.
`check` prints problems and warnings and exits 1 on any error-class problem.

`export` writes what `emit_programs` returns (#95). One program goes to
`-o`, or to the project's path with the post-processor's extension, as
before. Programs of tool runs go to `<stem>-<k>-T<n>.<ext>`, where `<k>` is
the run's 1-based place, `<n>` its tool's number, and `<stem>` and `<ext>`
are those of that one path (`-o out/job.nc` gives `out/job-1-T1.nc`). Every
program, and with `--post` the project, is staged before any is moved into
place, so a failure writes nothing. The summary then starts
`Wrote <n> programs; run them in this order:` and lists each file with
"Before you start: fit T{n} {name} and set work zero (X, Y and Z) with it."
for the first and "… and set Z zero with it. Keep X and Y zero." for the
others, then the operation lines, then per program its line count, extents
and pauses. A single program in `"m6"` mode that changes tools adds a line
saying the sender must stop at `M6`. `check` prints the extents of each
program under `Program <k> (T<n>):`.
`check` and `export` prefix each problem about one value with
`<dcv>:line:column:`, so every missing value is listed, located, in one run
before any toolpath is generated.

`export` prints, after the operation lines and before "Before you start",
and `check` prints, after its problems when the program could be built,
the program's extents (#62):

```
Extents (machine coordinates, mm, tool centre): X 6.825..103.175, Y 6.825..103.175, Z -12.000..5.000
  The cutter's edge reaches X 3.650..106.350, Y 3.650..106.350; keep clamps outside that.
```

Numbers go through `dc-gcode::format::format_number` with the
post-processor's number format, so they read like the program's words, in
the program's units (`mm` or `in`, the project's display units). Other
summary values (`12 mm deep`) are project values and stay in mm; the
extents are machine coordinates, which the user compares with the
controller's display, so they use the program's units. The second line
widens X and Y by the largest radius among the program's tools: the
exported operations' tools for one program, the run's tool for a tool
run's program.

## Module layout

```
crates/dc-geometry/src/{lib.rs, point.rs, polyline.rs, contour.rs, region.rs, distance.rs, offset.rs, flatten.rs, transform.rs}
crates/dc-project/src/{lib.rs, model.rs, ids.rs, serialize.rs, layout.rs, locate.rs, migrate.rs, validate.rs, history.rs, post_processor.rs, definitions/{README.md, grblhal/v1.json, grbl/v1.json}}
crates/dc-cam/src/{lib.rs, toolpath.rs, warning.rs, profile.rs, overlap.rs, passes.rs, stock.rs, tabs.rs}
crates/dc-gcode/src/{lib.rs, error.rs, post.rs (tests only), emit.rs, format.rs, transform.rs, parse.rs}
crates/dc-import/src/{lib.rs, svg.rs}
crates/dreamcarve/src/{main.rs, cli.rs, init.rs, export.rs, check.rs}
tests/golden/{profile-outside-rect, profile-inside-circle, profile-ring, profile-tabs-circle}/
```

## Test plan

Names are `crate::module::test`. "(port)" marks a PureCutCNC test to port;
"(golden)" a corpus comparison.

**dc-geometry**
- `polyline::closed_rect_has_expected_length` — 100×50 rect length 300.
- `contour::normalises_cw_input_to_ccw` — orientation flag true after construction.
- `contour::area_sign_matches_orientation` — CCW positive, CW negative.
- `contour::bowtie_is_rejected_as_self_intersecting` — `SelfIntersecting` at (180/7, 120/7).
- `contour::figure_eight_with_arcs_is_rejected` — two bulge arcs crossing once.
- `contour::simple_polygons_are_accepted` — square, concave star, collinear and repeated vertices.
- `offset::outside_rect_grows_bounds_by_diameter` — +3.175 on 100×50 → 106.35×56.35 bounds.
- `offset::inside_rect_shrinks_bounds_by_diameter` — −3.175 → 93.65×43.65.
- `offset::inside_offset_larger_than_shape_vanishes` — returns empty, no panic.
- `offset::round_joins_produce_arc_vertices` — outside offset of a rect has four bulge ≠ 0 vertices.
- `flatten::circle_within_tolerance` — every flattened point within 0.01 of radius.
- `flatten::preserves_closed_flag` — closed BezPath → closed polyline.
- `nest::circle_in_circle_becomes_hole` — one region, one hole, hole is CW.
- `nest::disjoint_shapes_are_separate_regions`.
- `nest::several_parts_each_get_their_own_hole`, `nest::shapes_touching_from_outside_do_not_cross`.
- `nest::crossing_circles_are_an_error`, `nest::crossing_rects_are_an_error_whatever_the_order` — `ContoursCross` with both input indices.
- `nest::identical_shapes_keep_the_first_and_warn`, `nest::duplicate_drawn_the_other_way_is_still_a_duplicate`, `nest::duplicate_hole_is_dropped_and_part_keeps_one_hole`.
- `point_at::tangent_is_unit_and_along_segment`.

**dc-project**
- `serialize::round_trip_is_identity` — load(save(p)) == p.
- `serialize::keys_are_stable` — saved JSON matches a committed fixture byte for byte.
- `serialize::settings_are_written_before_geometry`, `serialize::vertices_are_one_per_line`, `serialize::compact_output_round_trips`, `serialize::old_key_order_still_loads` (#76).
- `locate::paths_map_to_line_and_column`, `locate::positions_survive_reformatting` (#76).
- `migrate::version_0_fixture_loads` — a committed v0 file migrates (placeholder for the first real migration; v0 is the pre-release shape).
- `migrate::newer_version_is_rejected`.
- `validate::dangling_tool_id_is_error`.
- `validate::unset_feed_is_an_error` (a warning before #76), `validate::unused_tool_with_unset_values_is_a_warning_not_an_error`, `validate::tool_used_only_by_disabled_operation_is_unused`, `validate::used_tool_with_unset_values_is_still_an_error`.
- Unset step-down (#98): `validate::step_down_unset_on_operation_and_tool_is_error`, `validate::step_down_on_operation_only_is_valid`, `validate::step_down_on_tool_only_is_valid`, `validate::step_down_unset_reported_once_per_tool`, `validate::disabled_operation_needs_no_step_down`; CLI `init_leaves_operation_step_down_unset`, `check_after_init_lists_missing_step_down`.
- Sanity checks (#67): `validate::step_down_above_diameter_warns`, `validate::v_bit_in_profile_is_an_error`, `validate::ball_nose_and_engraver_in_profile_are_errors` (Outside and Inside), `validate::v_bit_on_line_is_allowed`, `validate::ball_nose_and_engraver_on_line_are_allowed`, `validate::feed_above_limit_is_an_error`, `validate::plunge_above_limit_is_an_error`, `validate::rpm_above_limit_warns`, `validate::plunge_above_feed_warns`, `validate::diameter_above_limit_warns`, `validate::empty_operation_is_an_error`, `validate::unused_tool_gets_no_sanity_findings`, `validate::sanity_problems_are_reported_together`, `validate::quick_start_values_have_no_sanity_findings`; CLI `quick_start_project_has_no_sanity_warnings`, `export_refuses_feed_above_limit_with_location`.
- `post_processor::post_processor_round_trips_through_json`, `post_processor::unknown_field_is_rejected`.
- Post-processor reference (#106): `post_processor::project_post_processor_round_trips`, `post_processor::missing_post_processor_setting_fails_to_load`, `post_processor::unknown_post_id_fails_to_load`, `post_processor::unknown_definition_version_fails_to_load`, `post_processor::resolve_matches_old_embedded_definition`, `post_processor::released_definition_versions_are_unchanged`; `migrate::version_6_fixture_loads`, `migrate::v6_with_bundled_copy_migrates_to_reference`, `migrate::v6_with_edited_dialect_fails_to_load`, `migrate::v6_with_unbundled_id_fails_to_load`; CLI `init_writes_post_processor_reference`, `export_post_switch_writes_reference`, and the byte-for-byte `manual_spindle_export_matches_committed_program`, `multi_tool_exports_match_committed_programs`, `m6_export_matches_committed_program`, `grbl_exports_match_committed_programs`.
- `history::undo_restores_previous_snapshot`, `history::redo_after_undo`, `history::new_edit_truncates_redo`.

**dc-cam**
- `passes::even_division` — 12/4 → [8, 4, 0].
- `passes::uneven_final_pass_is_shallower` — 12/5 → [7, 2, 0].
- `passes::depth_beyond_bottom_is_error`.
- `passes::through_cut_allowance_permits_exactly_that_much` — 12.2 with allowance 0.2 ok; 12.3 error.
- `profile::outside_rect_offsets_outward` — tool-centre bounds 106.35×56.35.
- `profile::inside_rect_offsets_inward`.
- `profile::ring_cuts_outer_outside_and_hole_inside` — two contours, outer bounds grow, hole bounds shrink.
- `profile::climb_outside_is_clockwise` — signed area of traversal negative.
- `profile::conventional_outside_is_counter_clockwise`.
- `profile::climb_inside_is_counter_clockwise`.
- `profile::open_path_is_skipped_with_warning`.
- `profile::tool_larger_than_hole_warns_and_skips`.
- `profile::v_bit_in_profile_is_an_error` — on Outside or Inside, any cutter but a flat end mill is `CamError::CutterNotFlat` (#67).
- `profile::v_bit_on_line_is_allowed` — On line, a V-bit, ball-nose or engraving bit cuts with no error or warning (#67).
- `profile::every_cut_move_has_feed_and_every_rapid_has_none`.
- `profile::first_move_is_rapid_at_safe_z`.
- `profile::descent_uses_plunge_feed`.
- `profile::retracts_to_safe_z_between_contours`.
- `profile::toolpath_off_stock_is_an_error` — outside cut 6.35 mm past a 120 mm stock is `OffStock` at +X; the inside cut fits (#62).
- `stock::rapids_do_not_count`, `stock::arc_bulge_counts`, `stock::edge_to_edge_is_on_the_stock` (#62).
- `tabs::count_four_spaces_evenly` — centres at L/8, 3L/8, 5L/8, 7L/8.
- `tabs::span_is_width_plus_diameter` (port: `tabs.test.ts` tool-centre footprint cases).
- `tabs::passes_above_tab_height_untouched`.
- `tabs::final_pass_has_exactly_four_raised_segments_at_tab_z`.
- `tabs::overlapping_tabs_merge_and_warn` (port).
- `tabs::smooth_tab_plateau_is_bit_diameter` — on a straight edge the final pass holds the tab top for at least one bit diameter, centred on the tab (#66).
- `tabs::smooth_tab_bridge_is_full_height` — the material left under the bit, computed from the moves and tool radius, peaks at `height_mm` within 0.01 mm (#66).
- `tabs::smooth_tab_bridge_is_triangle` — material at offset x from the tab centre is `height·(1 − |x|/(width/2))` within 0.01 mm, on a straight edge and on arcs (#66).
- `tabs::tabs_above_cut_depth_warn` (port: `applyTabWarnings`).

**dc-gcode**
- `post::bundled_definitions_validate` — every version of every definition parses and has its id; the latest are `grblhal` and `grbl`. The other `post::bundled_*` tests also run over every version.
- `post::latest_definition_by_id` — finds both; an unknown id is `PostProcessorError::UnknownId`, listing them.
- `post::bundled_line_number_increments_are_positive`, `post::bundled_default_settings_are_valid` (#106).
- `post::grblhal_and_grbl_differ_only_in_dialect` — same toolpath, diff restricted to header/footer/comment/tool-change/WCS lines.
- `transform::origin_bottom_left_stock_top_translates_only` — no coordinate changes sign.
- `transform::axis_inversion_from_definition_applies`.
- `format::decimal_places_and_trailing_zeros` (port: formatting cases in `postprocessor.test.ts`).
- `emit::missing_feed_is_error_naming_tool_and_field`.
- `emit::missing_rpm_is_error`.
- `emit::units_command_first_then_wcs`.
- `emit::spindle_on_with_rpm_before_first_cut`.
- `emit::arcs_emitted_as_g2_g3_when_supported` and `emit::arcs_flattened_when_not`.
- `emit::inch_project_scales_and_emits_g20`.
- `safety::no_z_below_bottom_minus_allowance` — S1, on every golden and unit toolpath.
- `safety::rapids_only_at_or_above_safe_z` — S2.
- `safety::every_cut_has_active_feed` — S3.
- `safety::violation_is_error_not_warning` — a hand-built bad toolpath is rejected.
- `safety::reversed_arc_fails_reproduction`, `safety::wrong_arc_centre_fails`,
  `safety::extra_move_between_points_fails`, `safety::flattened_arc_passes`,
  `safety::flipped_arcs_fail_emit` — S6 (#60).
- `safety::cut_off_stock_fails_s7` — S7: a cut along the stock edge with a 6 mm tool, and an arc bulging past the back edge, fail (#62).
- `safety::extents_include_arc_bulges` — `Program.extents` reaches an arc's top, past its end words (#62).
- `parse::round_trips_emitted_program` — parse(emit(tp)) reproduces tp's XYZ within 1e-3.

**dc-import**
- `svg::mm_sized_root_uses_declared_units`.
- `svg::unitless_root_assumes_96_dpi`.
- `svg::y_is_flipped` — a point near the top of the drawing has large y.
- `svg::transforms_are_applied` — translated group lands at translated coordinates.
- `svg::closed_and_open_subpaths_preserved`.
- `svg::circle_flattens_within_tolerance`.
- `svg::text_and_images_warn_and_skip`.
- Ported cases from `src/import/svg.test.ts` that concern path geometry (not UI classification).

**dreamcarve (integration, `tests/`)**
- `cli::init_then_export_coaster_matches_committed_program` — end to end on `fixtures/coaster.svg`, compared to a committed expected `.nc`.
- `cli::export_refuses_zero_feed_and_writes_nothing`.
- `cli::check_reports_open_paths`.
- `cli::init_lists_every_required_value_with_location`, `cli::check_reports_all_missing_values_at_once` (#76) — one `coaster.dcv:L:C:` line per unset value, each at its `null`.
- `cli::self_intersecting_target_fails_check` — `check` exits 1 naming the instance; `export` writes nothing.
- `cli::posts_lists_grblhal_and_grbl`.
- `cli::toolpath_off_stock_is_an_error` — the coaster with `e` = 80 fails `check` naming `op1` and 71.35 mm past +X; `export` writes nothing (#62).
- `cli::toolpath_just_inside_stock_passes` — the coaster, and moved 3.5 mm either way, pass (#62).
- `cli::export_prints_extents` — `export` and `check` print Z equal to the Z words' range and X, Y containing the X, Y words' ranges of `tests/expected/coaster-grblhal.nc` (#62).
- `cli::multi_tool_export_writes_one_file_per_tool_run`, `cli::single_tool_export_is_unchanged`, `cli::alternating_tools_keep_cut_order`, `cli::multi_tool_export_works_on_grbl`, `cli::output_flag_sets_the_stem`, `cli::failed_tool_run_writes_no_files`, `cli::m6_mode_writes_one_program`, `cli::m6_mode_on_grbl_is_invalid` — one program per tool run by default, `"m6"` opt-in on grblHAL (#95).
- `emit::tool_run_program_passes_safety_alone` — each program of `emit_programs` is `emit` of its run alone: no `M6` or `M61`, its own extents, deterministic (#95).

**dc-gcode (integration, `tests/golden.rs`)**
- `golden::profile_outside_rect` (golden), `golden::profile_inside_circle` (golden), `golden::profile_ring` (golden), `golden::profile_tabs_circle` (golden) — per-Z-level cutting move sets match within 0.01 mm after start-point alignment; feeds equal, except the differences each case's README lists.

## Slices

1. **Geometry core**: point, vertex, polyline, contour, transform, length,
   point_at, bounds. Tests: polyline, contour, point_at groups.
2. **Offset and flatten**: cavalier_contours and kurbo wrappers. Tests:
   offset, flatten groups.
3. **Nesting**: regions from contours. Tests: nest group.
4. **Project model and serialization**: types, ids, save/load, validate,
   version 1, migration scaffold with a v0 fixture. Tests: dc-project
   serialize, migrate, validate groups.
5. **History**: snapshot undo/redo. Tests: history group.
6. **Post-processor model**: serde struct, bundled JSON, validation,
   NOTICE entry. Tests: `post::*`.
7. **Depth passes and profile contour**: profile.rs, passes.rs without tabs.
   Tests: passes group, profile group.
8. **Tabs**: rect and smooth. Tests: tabs group. Depends on 7.
9. **Emission and safety parse**: emit.rs, format.rs, transform.rs,
   parse.rs. Tests: transform, format, emit, safety, parse groups. Depends
   on 6, 7.
10. **SVG import**. Tests: svg group. Depends on 2.
11. **CLI**: init, export, check, posts. Tests: cli group. Depends on 4, 9,
    10.
12. **Golden corpus**: capture four cases with `tools/parity/`, write the
    comparison harness. Tests: golden group. Depends on 9.
13. **Docs**: user guide chapters and quick start; contributor crate map.
14. **Real cut and release**: cut the coaster, record it, tag `v0.1.0`.

## Open questions

None. Resolved at acceptance (2026-10-15): `display_units` lives on the
project, because `init --units inch` must survive the user's edit cycle.
Revisit if it causes confusion.
