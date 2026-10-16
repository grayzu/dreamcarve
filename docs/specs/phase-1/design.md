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
                       dc-gcode::emit(project, toolpaths) ─▶ Program (text) + warnings
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
| `dc-cam::tabs` | `src/engine/toolpaths/tabs.ts` (`applyTabsToEdgeRoute`, `toolCentreContours`, `tabLayoutFreeFraction`, `applyTabWarnings`), `tabProfile.ts` (smooth tab cosine), `tabs.test.ts` | Tabs are specified per operation by count or explicit positions along the path; PureCutCNC's world-placed `Tab` rectangles arrive in phase 5 with the GUI. Smooth profile math ported as is. |
| `dc-cam::passes` | depth stepping inside `edge.ts` | Final pass clamps to depth, never deeper. |
| `dc-project::post_processor` | `src/engine/gcode/types.ts` (`MachineDefinitionSchema`) | Keys snake_case. Every field required (upstream zod defaults dropped) so a saved file shows every setting. Lives in `dc-project` because `Project` embeds it. |
| `dc-gcode::post` | `postprocessor.ts` (`runPostProcessor`), `utils.ts` (`projectToMachinePoint`, number formatting) | `projectToMachinePoint` loses its Y mirror; axis mapping and inversion kept. Canned cycles and coolant parsed but unused until phase 2. |
| `dc-gcode/definitions/*.json` | `src/engine/gcode/definitions/grblhal.json`, `grbl.json` | Copied as data, keys converted to snake_case. Attribution in `NOTICE` and `definitions/README.md`; the generator name in the header becomes DreamCarve's (`{date}` stays a placeholder filled at export). |
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
/// holes are CW (ADR-0001). Constructors normalise.
pub struct Contour(Polyline);

/// One or more contours forming a region: one outer, zero or more holes.
pub struct Region { outer: Contour, holes: Vec<Contour> }

/// 2D affine transform (wraps kurbo::Affine).
pub struct Transform(kurbo::Affine);

pub fn offset(c: &Contour, delta_mm: f64) -> Vec<Contour>;       // cavalier_contours parallel_offset; +delta grows
pub fn flatten(path: &kurbo::BezPath, tolerance_mm: f64) -> Vec<Polyline>;
pub fn nest(contours: Vec<Contour>) -> Result<(Vec<Region>, Vec<GeometryWarning>), GeometryError>;
    // even-odd containment → outer/holes; crossing shapes are ContoursCross, duplicates dropped with a warning
pub fn length(p: &Polyline) -> f64;
pub fn point_at(p: &Polyline, distance: f64) -> (Point, Vec2);    // position and tangent
pub fn bounds(p: &Polyline) -> Rect;
```

Errors: `GeometryError::{Degenerate, SelfIntersecting, OffsetVanished, ArcUnderNonUniformTransform, ContoursCross}`.
Warnings: `GeometryWarning::DuplicateContourDropped`. The nesting rules, with
figures, are in the user guide chapter "Shapes, holes, and islands".

### dc-project

```rust
pub const CURRENT_VERSION: u32 = 1;

pub struct Project {
    pub version: u32,
    pub name: String,
    pub display_units: Units,                 // Mm | Inch; affects CLI and init only
    pub flatten_tolerance_mm: f64,            // default 0.01
    pub stock: Stock,
    pub origin: MachineOrigin,
    pub definitions: BTreeMap<DefinitionId, FeatureDefinition>,
    pub instances: Vec<FeatureInstance>,
    pub tools: Vec<Tool>,
    pub operations: Vec<Operation>,
    pub post_processor: PostProcessor,        // embedded copy, see dc-project::post_processor
}

pub struct Stock { pub width_mm: f64, pub height_mm: f64, pub thickness_mm: f64 }
// Stock occupies [0,width]×[0,height]×[0,thickness] in project space.

pub struct MachineOrigin {
    pub xy: OriginXy,     // BottomLeft | BottomRight | TopLeft | TopRight | Centre | Custom{x,y}
    pub z: OriginZ,       // StockTop | StockBottom
}

pub struct FeatureDefinition { pub id: DefinitionId, pub name: String, pub profiles: Vec<Polyline> }

pub struct FeatureInstance {
    pub id: InstanceId, pub definition: DefinitionId, pub transform: Transform,
    pub name: String, pub visible: bool, pub locked: bool,
    pub z_top_mm: f64, pub z_bottom_mm: f64,
}

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
    pub depth_mm: f64,                      // from stock top
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
`load(bytes) -> Result<Project, LoadError>` reads `version`, applies
migrations in sequence up to `CURRENT_VERSION`, then validates with
`validate(&Project) -> Vec<Problem>` (dangling ids, non-positive dimensions,
an unsafe post-processor `file_extension`; disabled operations, unset tool
feeds and open profiles as warnings). Tool problems name the tool by name
and id. A file with a newer version
fails with `LoadError::NewerVersion`.

### dc-cam

```rust
pub struct Toolpath { pub moves: Vec<Move> }
pub enum MoveKind { Rapid, Linear, ArcCw { centre: Point }, ArcCcw { centre: Point } }
pub struct Move { pub kind: MoveKind, pub to: Point3, pub feed_mm_min: Option<f64> }
// Rapids carry no feed. Every Linear/Arc carries Some(feed).

pub struct Warning { pub code: WarningCode, pub params: BTreeMap<String, String> }

pub fn generate_profile(project: &Project, op: &ProfileOp) -> Result<(Toolpath, Vec<Warning>), CamError>;
```

### dc-gcode

`PostProcessor` is a serde struct mirroring PureCutCNC's `MachineDefinition`
schema field for field (id, name, description, vendor, builtin,
file_extension, coordinate_system, number_format, units, program,
work_coordinates, motion, feed_speed, tool_change, canned_cycles, coolant,
stop). It lives in `dc-project::post_processor`, not here, because `Project`
embeds it and the crate direction (`dc-geometry → dc-project → dc-gcode`)
forbids `dc-project` depending on `dc-gcode`; `dc-gcode` re-exports it.
`dc-gcode::post` holds the bundled definitions: `include_str!` JSON parsed at
first use and validated by a test, with `bundled()` and
`lookup(id) -> Result<PostProcessor, ExportError>`.

```rust
pub struct Program { pub text: String, pub extension: String, pub pauses: Vec<Pause> }
pub struct Pause { pub line: usize, pub message: String }
pub fn emit(project: &Project, ops: &[(ProfileOp, Tool, Toolpath)]) -> Result<(Program, Vec<Warning>), ExportError>;
pub fn parse_motion(text: &str) -> Vec<MachineMove>;   // test-only helper, re-exported for safety tests
```

`ExportError::{MissingFeed{tool, field}, MissingRpm{tool}, BelowMaterialBottom{op, z}, RapidBelowSafeZ, NoEnabledOperations, UnknownPostProcessor{id}, BundledDefinition{file, source}}`.
`BundledDefinition` means a bundled JSON file failed to parse, which only a
broken build can cause; it keeps `dc-gcode` free of panics.

## Algorithms

### Profile contour (dc-cam::profile)

Inputs: project, op. Output: toolpath in project space.

1. Resolve each target instance: apply `transform` to every profile of its
   definition. Closed polylines become `Contour`s (normalised CCW). Open
   polylines raise `Warning::OpenPathSkipped` and are dropped.
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
   feed, traverse at feed, descend at plunge feed. Smooth tabs: ramp up
   and down at feed following `smoothTabZAt` (raised cosine), sampled per
   `smoothTabSampleFractions` at `flatten_tolerance_mm`. Tab spans that
   overlap merge. Warnings: `TabsOverlap`, `TabsOutsideCutRange`
   (ported from `applyTabWarnings`).
8. Return moves and warnings.

### Emission (dc-gcode::emit)

1. Preconditions: every referenced tool has `feed`, `plunge`, `rpm`, else
   `ExportError::MissingFeed`/`MissingRpm`. With
   `feed_speed.spindle_control: manual`, `rpm` is optional. At least one
   enabled op.
2. Compute machine transform from `origin`: translation `(−ox, −oy, −oz)`
   with `oz = thickness` for `StockTop`, 0 for `StockBottom`; then axis
   mapping and sign from `coordinate_system`. No mirror of Y.
3. Units: if `post.units` wants inches (determined by the project's
   `display_units`, which `init` and `--post` keep aligned), scale by 1/25.4
   after the transform, and emit `inchCommand`; otherwise `mmCommand`.
4. Header lines with placeholders `{programName}`, `{date}`, `{unitsCommand}`,
   `{wcsCommand}`, then `workCoordinates.selectCommand` if not in the header.
5. Per operation: `operationHeader` with `{operationIndex}`,
   `{operationName}`, `{toolNumber}`, `{toolName}`; tool change block on the
   first op or tool change; spindle on with `S{rpm}`; then
   `feed_speed.spindle_on_dwell.command` with `{seconds}` filled, unless its
   `duration_s` is 0 (after every spindle start, including after a tool
   change); then moves. After the last op of a tool, `spindle_off` before a
   tool change, and at the end unless the footer has it.
   With manual spindle control: no spindle words and no dwell. Lines of the
   tool-change block and footer that only stop the spindle are left out, and
   so are tool-change lines that only pause (`M0`, `M1`) and
   `pause_after_change`. Before the first move of each tool, retract to safe
   Z, then one pause for the tool change and the router:
   `M0 (MSG, Fit T{n} {name}. Turn the router ON. Speed {rpm} RPM. Resume to start cutting.)`,
   leaving out the speed when the tool has none and the `Fit` part when the
   tool-change block had no pause; the name is shortened so the message
   stays within 96 characters. After the last op of a tool
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
   bottom − allowance), S2 (rapids at or above machine safe Z), S3 (every
   G1/G2/G3 has an active F), S4 (spindle running and dwelled before a cut,
   or with manual control an `M0` at safe Z after the last retract before
   each tool's first cut, and no `M3`, `M5` or `S` word). A violation is
   `ExportError`, never a warning.
8. `Program.pauses` lists every `M0` line with its 1-based line number and
   its comment text, so the CLI can tell the user what each stop is for when
   the sender does not show the message.

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
   warning.
4. Each definition gets one instance with identity transform, `z_top =
   thickness`, `z_bottom = 0`.

Text, images, gradients, and clip paths are ignored with a warning. usvg
must be ≥ 0.45 (Apache/MIT; earlier versions are MPL-2.0).

### CLI (dreamcarve)

```
dreamcarve init <svg> [-o <dcv>] [--post grblhal|grbl] [--units mm|inch]
dreamcarve export <dcv> [-o <nc>] [--post <id>]
dreamcarve check <dcv>
dreamcarve posts
```

`init` builds a project: stock = drawing bounds plus 10 mm margin each side,
thickness 12 mm, origin BottomLeft/StockTop, one tool "Set me" with
`diameter 6.35`, flutes 2, feeds `None`, one Profile op targeting every
closed instance, outside, depth 12, step 4, climb, safe Z 5, tabs
`Count(4)` 8×3 rect. Exit codes: 0 ok, 1 validation/export error, 2 usage.
`check` prints problems and warnings and exits 1 on any error-class problem.

## Module layout

```
crates/dc-geometry/src/{lib.rs, point.rs, polyline.rs, contour.rs, region.rs, offset.rs, flatten.rs, transform.rs}
crates/dc-project/src/{lib.rs, model.rs, ids.rs, serialize.rs, migrate.rs, validate.rs, history.rs, post_processor.rs}
crates/dc-cam/src/{lib.rs, toolpath.rs, warning.rs, profile.rs, passes.rs, tabs.rs, tab_profile.rs}
crates/dc-gcode/src/{lib.rs, error.rs, post.rs, emit.rs, format.rs, transform.rs, parse.rs, definitions/{README.md, grblhal.json, grbl.json}}
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
- `migrate::version_0_fixture_loads` — a committed v0 file migrates (placeholder for the first real migration; v0 is the pre-release shape).
- `migrate::newer_version_is_rejected`.
- `validate::dangling_tool_id_is_error`.
- `validate::unset_feed_is_warning_not_error`.
- `post_processor::post_processor_round_trips_through_json`, `post_processor::unknown_field_is_rejected`.
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
- `profile::every_cut_move_has_feed_and_every_rapid_has_none`.
- `profile::first_move_is_rapid_at_safe_z`.
- `profile::descent_uses_plunge_feed`.
- `profile::retracts_to_safe_z_between_contours`.
- `tabs::count_four_spaces_evenly` — centres at L/8, 3L/8, 5L/8, 7L/8.
- `tabs::span_is_width_plus_diameter` (port: `tabs.test.ts` tool-centre footprint cases).
- `tabs::passes_above_tab_height_untouched`.
- `tabs::final_pass_has_exactly_four_raised_segments_at_tab_z`.
- `tabs::overlapping_tabs_merge_and_warn` (port).
- `tabs::smooth_profile_is_raised_cosine` (port: `tabProfile` tests for `smoothTabHeightFraction`, `smoothTabZAt`, sample counts).
- `tabs::tabs_above_cut_depth_warn` (port: `applyTabWarnings`).

**dc-gcode**
- `post::bundled_definitions_validate` — grblhal.json and grbl.json parse; ids are `grblhal` and `grbl`.
- `post::lookup_by_id` — finds both; an unknown id is `UnknownPostProcessor`.
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
- `cli::posts_lists_grblhal_and_grbl`.

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
