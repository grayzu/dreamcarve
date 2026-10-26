# Phase 3: V-Carve, Inlay, and Photo V-Carve — Design Spec

**Status:** draft
**Functional spec:** `functional.md`

## Overview

Phase 3 adds three generators to `dc-cam` (V-Carve, Inlay, Photo V-Carve),
a countersink cycle to Drill, and a clearing mode to Pocket. The medial
axis they rest on is a new `dc-geometry` module ported from PureCutCNC's
`vcarveMedial/`, on a Rust port of the same Delaunay library. Pictures
come in through `dc-import` and live in the project as **bitmaps**, a new
list beside the instances. `dc-gcode` gains the cone test (S17), the
countersink rule (S18) and the groove rule (S19). The CLI gains `init` on
a picture and an `image` command. The format moves to version 16.

```
SVG / DXF ──dc-import──▶ FeatureDefinitions ─┐
PNG / JPEG ─dc-import::bitmap─▶ Bitmap ───────┼─▶ Project (.dcv v16, dc-project)
user edits .dcv ─────────────────────────────┘        │
                                                      ▼
   dc-cam::{profile, pocket (+ clear_for), drill (+ countersink),
            engrave, vcarve (medial | offset), inlay, photo}::generate
                         │  Vec<CutBlock> + Vec<Warning>
                         ▼
   dc-cam::sequence ─▶ Toolpath ─▶ dc-cam::order::check (+ start_depth_mismatch)
                         │
                         ▼
   dc-gcode::emit_programs ─▶ safety pass (S1–S16, + S17 cone, S18 countersink, S19 grooves)
```

Everything is millimetres, Y-up, Z-up, stock bottom at Z = 0 (ADR-0001).
The medial axis is the one piece of this phase whose port needs no Y-up
re-derivation: a Voronoi skeleton is the same under reflection. What does
change hands is stated where it happens: the offset strategy's ring
direction (as Pocket's), a picture's row order (its top row is at the
highest Y), and the male Inlay's mirror (about X, in place).

Three things in this phase are **not** ports:

- **Clearing Pockets and the V-bit floor** (functional 39, 40) follow
  VCarve's "Use Clearance Tools" and Corbin's pocket setup; PureCutCNC
  clamps depth but clears nothing.
- **Inlay** (45) follows Corbin's Workshop's method; it is composed from
  the V-Carve generator and the geometry crate.
- **Photo V-Carve** (47) is designed from RasterCarve's documented
  behaviour and its command-line defaults. RasterCarve is GPLv2+: no line
  of it is read for porting, nothing from it is copied, and the only thing
  of its that enters the repository is the G-code its command line writes
  on DreamCarve's own fixture pictures, captured as goldens the way
  PureCutCNC's programs are.

## PureCutCNC sources

| DreamCarve | PureCutCNC | Port notes |
| --- | --- | --- |
| `dc-geometry::medial` | `src/engine/toolpaths/vcarveMedial/medialAxis.ts` (`computeMedialAxis`, `resampleLoop`, `SegmentDistanceGrid`, `hasContactSpread`, `regionConvexCorners`, `contractMicroClusters`, `pruneSpurs`, `dropSmallComponents`, `extendIntoCorners`, `finalizeGraph`), `resolution.ts` (`resolveMedialResolution`, the 160-sample and 40 000-sample rules) | Ported as geometry on `Region`, not on `Point[]` loops: arcs are flattened once at the project tolerance before sampling (settled decision: no re-flattening in generators; the one place a carve needs chords is here, and it samples at the medial resolution anyway). `delaunator` (Rust) replaces `delaunator` (JS); it is the same author's same algorithm, so triangulations agree. Constants kept: prune factor 1.5, corner angle 50°, contact spread 60°, cluster cell 0.75 × resolution, small component 4 × resolution, tolerance ratio `0.003 + (res/clearance)²/8`. The `Map<string, …>` keys of rounded coordinates become integer-pair keys in a `BTreeMap` (determinism). |
| `dc-cam::vcarve::medial` | `vcarveMedial/toolpath.ts` (`extractChains`, `pruneRedundantLeafChains`, `simplifyChain`, `emitMedialToolpath`), `vcarveMedial/index.ts` (`generateVCarveMedialToolpath`: band loop, auto-refinement, nearest region first) | Chains become `CutBlock`s and go through `sequence`; the stay-down link rule (`LINK_CLEARANCE_FRACTION` 0.9 of the carved disc at the clamped depth) becomes the block's `cleared` region so `sequence::link_inside_cleared` decides. Bands are gone (one stock, no per-instance Z); the start plane replaces `band.topZ`. `simplifyTolerance` 0.25 × resolution and `redundancyTolerance` 0.75 × resolution kept. The flat floor trace and the V-bit floor are new (VCarve). |
| `dc-cam::vcarve::offset` | `src/engine/toolpaths/vcarve.ts` (`generateVCarveToolpathSingle`: `buildInsetRegions` rings at `currentDepth · slope`, `sortContoursNearestNeighbor`, the 8 × stepover cross-Z link budget) | Rings from `dc-geometry::offset` (cavalier_contours, round joins as upstream's `jtRound`). Ring direction re-derived as Pocket's `ring_is_ccw`. The cross-Z stay-down link becomes a block link that `sequence` allows only inside `cleared`. |
| `dc-cam::drill` countersink | `src/engine/toolpaths/drilling.ts` (`countersinkTipDepth`, `resolveCountersinkDepth`, `emitCountersinkPlunge`, the mouth-vs-hole skip), `drillingCountersink.test.ts` | Depth from the start plane (functional 44), not from a feature's own top face. The `maxCutDepth` check becomes the cone-height check from the tool's diameter (DreamCarve tools have no max cut depth). Never a `DrillHole` cycle record (stays `Expanded`). |
| `dc-cam::vcarve` tests | `vcarveMedial/vcarveMedial.test.ts` (22 tests), `resolution.test.ts` (5), `noiseRegression.test.ts` (2) | Ported; the text fixtures ("Rag", "g", the `gA` project) are captured as polygon JSON by the parity tool from PureCutCNC's resolver, since DreamCarve has no fonts until phase 5. |
| `tests/golden/vcarve-*` | captured via `tools/parity/` from `v_carve_medial` and `v_carve` operations | See "Golden corpus". |

Files in the first column that port logic carry the upstream attribution
header. `dc-cam::inlay`, `dc-cam::photo`, `dc-import::bitmap`,
`dc-project::bitmap` and the clearing-Pocket code are DreamCarve's and
carry the DreamCarve header.

## Data model

### dc-project (format version 16)

```rust
pub const CURRENT_VERSION: u32 = 16;

pub struct Project {
    // phase 2 fields unchanged, plus:
    /// Pictures placed on the stock (functional 46), saved after
    /// `instances`. `init` on a drawing: empty; migration: empty.
    pub bitmaps: Vec<Bitmap>,
}

/// A grayscale picture placed on the stock. The pixels travel with the
/// project (ADR-0002), as an 8-bit grayscale PNG in base64.
pub struct Bitmap {
    pub id: BitmapId,            // "b<n>"; unique in the project
    pub name: String,            // the file's stem by default
    pub width_px: u32,           // from the PNG; validation checks they agree
    pub height_px: u32,
    pub x_mm: f64,               // bottom-left corner on the stock, project space
    pub y_mm: f64,
    pub width_mm: f64,           // footprint; `height_mm / width_mm == height_px / width_px`
    pub height_mm: f64,          //   within 1e-6 (validation), so the picture is never stretched
    pub visible: bool,           // GUI, reserved (as FeatureInstance's)
    pub locked: bool,
    /// 8-bit grayscale PNG, base64 (standard alphabet, padded). Written
    /// by DreamCarve's own encoder at a fixed compression level, so the
    /// same pixels always give the same text.
    pub png_base64: String,
}
impl Bitmap {
    /// The pixels, row-major from the top row of the picture, 0 black to
    /// 255 white. Decodes on each call (no cache in the model; `Clone`
    /// stays cheap and correct).
    pub fn pixels(&self) -> Result<Grayscale, LoadError>;
}
pub struct Grayscale { pub width_px: u32, pub height_px: u32, pub bytes: Vec<u8> }

pub enum Operation {
    Profile(ProfileOp), Pocket(PocketOp), Drill(DrillOp), Engrave(EngraveOp),
    /// Medial-line V-carving of closed shapes (functional 38–43).
    VCarve(VCarveOp),
    /// One side of a V-bit inlay (functional 45).
    Inlay(InlayOp),
    /// A picture as parallel V-bit grooves (functional 47).
    PhotoVCarve(PhotoVCarveOp),
}

pub struct VCarveOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>,         // closed outlines; nested even-odd
    pub tool: ToolId,                     // must be a V-bit (validation, and the generator)
    pub strategy: VCarveStrategy,         // "medial" | { "offset": { "step_over_mm": …, "direction": … } }
    /// Flat depth, mm below the start plane; `None` carves to full depth
    /// (functional 39; VCarve "Flat Depth").
    pub flat_depth_mm: Option<f64>,
    /// The start plane, mm below the stock top, 0 or more (functional 41).
    pub start_depth_mm: f64,
    /// Step-over of the V-bit floor as a fraction of the cut width at the
    /// flat depth, in (0, 1]; overrides `Tool::step_over_fraction`.
    pub step_over_fraction: Option<f64>,
    pub machining_order: MachiningOrder,
    pub through_cut_allowance_mm: f64,    // S1, as every operation
    pub safe_z_mm: f64,
}

pub enum VCarveStrategy {
    /// Along the medial lines (default; `init` and the chapter write it).
    Medial,
    /// Concentric rings stepping in by `step_over_mm` (functional 43).
    Offset { step_over_mm: f64, direction: CutDirection },
}

pub struct InlayOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>,
    pub tool: ToolId,
    pub side: InlaySide,                  // "female" | "male"
    pub depth_mm: Option<f64>,            // Corbin `inlay_depth`; None until set
    pub glue_gap_mm: Option<f64>,         // Corbin `inlay_fit_adjustment`
    pub plug_depth_mm: Option<f64>,       // Corbin `plug_depth`
    pub margin_mm: Option<f64>,           // Corbin plug stock offset; male only
    pub step_over_fraction: Option<f64>,  // V-bit floor, as VCarveOp
    pub machining_order: MachiningOrder,
    pub through_cut_allowance_mm: f64,
    pub safe_z_mm: f64,
}
pub enum InlaySide { Female, Male }

pub struct PhotoVCarveOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub image: BitmapId,
    pub tool: ToolId,                     // V-bit
    pub max_depth_mm: Option<f64>,        // VCarve "Max Carving Depth"; None until set
    pub start_depth_mm: f64,
    pub line_angle_deg: f64,              // [0, 180)
    pub line_spacing_fraction: f64,       // > 0; < 1 warns
    pub strategy: PhotoStrategy,          // "raster" | "hatch"
    pub invert: bool,
    pub sample_mm: f64,                   // > 0; `init` writes 0.25
    pub stepover_retract_mm: f64,         // > 0; `init` writes 1
    pub through_cut_allowance_mm: f64,
    pub safe_z_mm: f64,
}
pub enum PhotoStrategy { Raster, Hatch }

pub struct PocketOp {
    // phase 2 fields unchanged, plus:
    /// A V-Carve or Inlay this Pocket clears the flat floor for
    /// (functional 40). Exclusive with `rest_of`. With it, `targets` is
    /// empty and `depth_mm` is `None`: both come from the carve.
    pub clear_for: Option<OperationId>,
}

pub enum DrillCycle {
    Simple,
    Peck { peck_depth_mm: Option<f64>, retract: PeckRetract },
    /// A V-bit opens a mouth of `diameter_mm` at the start plane
    /// (functional 44). Saved as `{ "countersink": { "diameter_mm": 6.0,
    /// "start_depth_mm": 0.0 } }`.
    Countersink { diameter_mm: f64, start_depth_mm: f64 },
}
```

Serialization: `VCarveStrategy` has a unit variant, so it is externally
tagged (`"medial"` or `{ "offset": { … } }`), like `DrillCycle`. `InlaySide`
and `PhotoStrategy` are plain strings. `Operation` keeps its internal
`kind` tag: `"v_carve"`, `"inlay"`, `"photo_v_carve"`. `Bitmap::png_base64`
is the one long string in a `.dcv`; the formatter writes it on one line,
as it writes a vertex.

**Migration v15 → v16** (`migrate::v15_to_v16`): insert `"bitmaps": []`
after `instances`; add `"clear_for": null` after `rest_of` in every Pocket.
Nothing else changes, so a v15 file exports the same bytes. Fixture
`crates/dc-project/tests/fixtures/dcv/v15/sample.dcv` (the plaque with a
Pocket).

**Validation** (`validate.rs`), errors unless marked:

- `v_carve_needs_v_bit` (V-Carve, Inlay, Photo V-Carve, countersink Drill
  whose tool is not `ToolKind::VBit`; the Drill form is
  `countersink_needs_v_bit`). Located at `operations[i].tool`.
- `v_bit_angle_range`: the V-bit's `included_angle_deg` not in (0, 180),
  on every tool a V operation uses (the library already enforces it on
  import; a hand-edited project may not).
- `operation_value_unset` extended to `flat_depth_mm`'s neighbours that are
  required: Inlay `depth_mm`, `glue_gap_mm`, `plug_depth_mm`, and
  `margin_mm` on a male; Photo `max_depth_mm`.
- `v_bit_cone_exceeded` (functional 39, 44, 47): a V-Carve or Inlay whose
  flat depth (or, with none, the deepest medial clearance: generator-time
  only, since validation has no geometry; validation checks only the
  flat depth and the Photo `max_depth_mm` and the countersink's derived
  depth) is deeper than `diameter_mm / (2 · tan(α/2))`. The generator
  repeats the check on the actual depth (see "V-Carve generator").
- `clear_for_after_carve`, `clear_for_needs_flat_depth`,
  `clear_for_has_targets` (`targets` not empty or `depth_mm` not null),
  `clear_for_with_rest_of`, `clear_for_unknown` (names no enabled V-Carve
  or Inlay). A clearing Pocket's tool must still be a flat end mill
  (`cutter_not_flat`, as any Pocket).
- `countersink_mouth_exceeds_tool` (`diameter_mm > tool.diameter_mm`),
  `countersink_diameter_positive`, `start_depth_negative` (any
  `start_depth_mm` below 0, located at the field).
- `inlay_glue_gap_not_below_depth` (`glue_gap_mm >= depth_mm`),
  `inlay_plug_base_thin` (warning: male with `thickness − (depth − gap +
  plug) < 6`), `inlay_margin_positive` (male with `margin_mm <= 0`).
- `photo_line_angle_range` ([0, 180)), `photo_line_spacing_positive`,
  `photo_lines_overlap` (warning: `line_spacing_fraction < 1`),
  `photo_sample_positive`, `photo_retract_positive`,
  `photo_unknown_bitmap`, `bitmap_footprint_aspect` (`height_mm /
  width_mm` off the pixel aspect), `bitmap_pixels_mismatch` (PNG size
  differs from `width_px`/`height_px`), `bitmap_not_grayscale_8` (the PNG
  is not 8-bit grayscale: DreamCarve wrote it, so a hand edit is the
  only way), `duplicate_bitmap_id`.
- `drill_cutter_unusual` does not fire for a countersink Drill (its tool
  is a V-bit on purpose).

### dc-cam

```rust
pub fn generate_vcarve(project: &Project, op: &VCarveOp) -> Result<(Toolpath, Vec<Warning>), CamError>;
pub fn generate_inlay(project: &Project, op: &InlayOp) -> Result<(Toolpath, Vec<Warning>), CamError>;
pub fn generate_photo(project: &Project, op: &PhotoVCarveOp) -> Result<(Toolpath, Vec<Warning>), CamError>;
// generate_pocket and generate_drill gain the clear_for and countersink paths.

pub mod vcarve {
    /// What a V-bit carve is, independent of which operation asked for
    /// it: V-Carve builds one from its targets, Inlay one per side.
    pub struct Carve<'a> {
        pub regions: Vec<Region>,          // carved area, even-odd nested, project space
        pub start_plane_z_mm: f64,         // thickness − start_depth
        pub flat_depth_mm: Option<f64>,
        pub strategy: &'a VCarveStrategy,
        pub step_over_fraction: f64,       // V-bit floor
        pub cleared_by: Vec<Clearing>,     // earlier clearing Pockets' reach (see "Clearing Pocket")
    }
    pub struct Clearing { pub radius_mm: f64 }
    /// The floor region of a carve: its area inset by `flat · tan(α/2)`,
    /// at `start_plane − flat`. `None` without a flat depth.
    pub fn floor_regions(carve: &Carve, half_angle_rad: f64, tolerance_mm: f64) -> Option<Vec<Region>>;
    /// Blocks for one carve with one V-bit; the generators sequence them.
    pub fn blocks(project: &Project, op: &OperationId, tool: &Tool, carve: &Carve, warnings: &mut Vec<Warning>) -> Result<Vec<CutBlock>, CamError>;
    pub fn cone_height_mm(tool: &Tool) -> Option<f64>;   // diameter / (2 tan(α/2)); None for a non-V-bit
}

pub mod inlay {
    /// The male's mirror: X reflected about the targets' joint bounding
    /// box centre line, in project space.
    pub fn male_mirror(project: &Project, targets: &[InstanceId]) -> Result<Transform, CamError>;
    /// The surround: the mirrored design's outer outlines offset out by
    /// `margin_mm`, united, minus the mirrored design.
    pub fn surround(design: &[Region], margin_mm: f64, tolerance_mm: f64) -> Vec<Region>;
}

pub mod photo {
    /// The groove family: unit direction, perpendicular spacing, and the
    /// clipped grooves in cut order with their sample points.
    pub struct Grooves { pub direction: Vec2, pub spacing_mm: f64, pub lines: Vec<Vec<Point>> }
    pub fn grooves(footprint: Rect, angle_deg: f64, spacing_mm: f64, sample_mm: f64) -> Grooves;
    /// Brightness 0..=255 at a project point, bilinear between pixel
    /// centres, clamped at the edges.
    pub fn sample(bitmap: &Bitmap, pixels: &Grayscale, p: Point) -> f64;
}
```

New `WarningCode`s (params in parentheses): `VCarveTargetOpen (op,
instance, instance_name, profile)`, `VCarveNoMedialAxis (op, instance,
instance_name, profile)` (a region so thin that no interior node survives
two refinements; upstream `vcarveNoMedialAxis`), `VCarveSamplingBudget
(op, resolution_mm)` (once per operation; upstream `vcarveSamplingBudget`),
`ClearForNoRoom (op, clear_for, tool, diameter_mm)`, `StartDepthMismatch
(op, pocket: Option<OperationId>, start_depth_mm, pocket_depth_mm:
Option<f64>)`, `CountersinkMouthNotWiderThanHole (op, instance,
instance_name, profile, mouth_mm, hole_mm)`, `InlayMaleMirrored (op,
axis_x_mm)`, `InlayPlugBaseThin (op, base_mm)` (generator-time too, from
the measured thickness), `PhotoLinesOverlap (op, fraction)`. New
`CamError`s: `VCarveNeedsVBit { op, tool, tool_id, cutter }`,
`VBitAngleRange { tool, tool_id, angle_deg }`, `VBitConeExceeded { op,
instance, profile, depth_mm, cone_mm }`, `ClearForInvalid { op, clear_for,
reason }` (the validation reasons, re-checked), `CountersinkMouthExceedsTool
{ op, mouth_mm, tool_mm }`, `InlayGlueGap { op, glue_gap_mm, depth_mm }`,
`InlayMargin { op, margin_mm }`, `PhotoBitmapUnknown { op, bitmap }`,
`PhotoSetting { op, field, value }`, `BitmapDecode { bitmap, reason }`.

### dc-gcode

```rust
pub enum ExportError {
    // phase 2 variants, plus:
    /// S17: a V-bit's cone crosses a wall of its carved region below the
    /// start plane, or its tip is below the flat depth, the maximum depth
    /// or the cone height.
    ConeOffMaterial { op: OperationId, line: usize, x_mm: f64, y_mm: f64, z_mm: f64, overlap_mm: f64 },
    /// S18: a countersink plunge ends somewhere other than the derived
    /// depth at a target's centre, or the program writes it as a cycle.
    CountersinkDepth { op: OperationId, line: usize, expected_mm: f64, found_mm: f64 },
    /// S19: a groove sample outside the picture, or below the maximum
    /// depth, or a between-groove move written as a rapid.
    GrooveOffPicture { op: OperationId, line: usize, x_mm: f64, y_mm: f64 },
    GrooveLinkRapid { op: OperationId, line: usize },
}
```

`material.rs` gains the three checks (see "Safety pass"). `EmitOp` is
unchanged: a V operation's toolpath is a `Toolpath` like a Pocket's;
`holes` stays `None` for a countersink Drill, which is what keeps the
emitter on the plain-moves path (S18; upstream does the same by recording
no cycle).

### dc-import

```rust
pub mod bitmap {
    /// Decodes a PNG or JPEG into an 8-bit grayscale picture: colour by
    /// Rec. 601 luminance, alpha composited on white, 16-bit by its high
    /// byte. `format` is by file extension, checked against the magic
    /// bytes. Returns the PNG DreamCarve stores and the warnings
    /// (`ImageColourConverted`, `ImageAlphaFlattened`, `Image16Bit`).
    pub fn import(bytes: &[u8], format: ImageFormat) -> Result<(Grayscale, Vec<ImportWarning>), ImportError>;
    pub enum ImageFormat { Png, Jpeg }
    /// The PNG `Bitmap::png_base64` holds: 8-bit grayscale, no
    /// interlace, deflate at the fixed level `BITMAP_PNG_COMPRESSION`,
    /// so re-encoding the same pixels gives the same bytes.
    pub fn encode(pixels: &Grayscale) -> Result<Vec<u8>, ImportError>;
}
```

`ImportError` gains `ImageDecode { reason }`, `ImageTooLarge { width_px,
height_px, limit_px }` (either side above `MAX_BITMAP_SIDE_PX` = 8192: a
picture that size is 64 MB of pixels and far beyond what a carve samples),
`ImageFormatMismatch { extension, found }`.

### CLI

```
dreamcarve init <svg|dxf|png|jpg|jpeg> [-o] [--post] [--units] [--width <mm> | --height <mm>] ...
dreamcarve image add <dcv> <png|jpg|jpeg> (--width <mm> | --height <mm>) [--at <x>,<y>] [--name <name>]
dreamcarve image list <dcv>
```

`init` on a picture requires exactly one of `--width`/`--height` (exit 2
otherwise, naming both flags); it writes the bitmap at the origin plus
the `STOCK_MARGIN_MM` shift `init` gives a drawing, a stock sized round
it, one V-bit tool (`t1`, 60°, 6.35 mm, feeds unset, as `init`'s end mill
is), and one Photo V-Carve operation with `max_depth_mm: null`,
`start_depth_mm: 0`, `line_angle_deg: 22.5`, `line_spacing_fraction: 1.1`,
`strategy: raster`, `invert: false`, `sample_mm: 0.25`,
`stepover_retract_mm: 1`, `safe_z_mm` as `init`'s. The report names the
values to set (thickness, max depth, the tool's feeds), as for a drawing.
`init` on a drawing writes `bitmaps: []` and no V operation: the user adds
one, following the chapter's JSON block, as for Pocket and Drill.

`image add` loads the project, imports the picture, appends a `Bitmap`
with the next free `b<n>` id, the file stem as the name (or `--name`),
the footprint from the flag and the pixel aspect, the corner from `--at`
(default `0,0`), saves through `dc_project::save` (placing tab requests
first, as every saving command does), and prints the bitmap's id, pixel
size, footprint and any import warning. `image list` prints a table `ID
NAME PX FOOTPRINT_MM AT_MM USED_BY` (`USED_BY`: the Photo V-Carve
operations naming it, `-` for none).

`check` and `export` print, for a V-Carve or Inlay, the deepest cut and
whether a flat floor exists and who clears it; for a Photo V-Carve, the
picture's footprint, the groove count and the full-depth groove width;
and once per male Inlay the `inlay_male_mirrored` note with the mirror
line's X. `--preview` draws each bitmap as an `<image>` element (the
stored PNG, inline) under the toolpaths, at its footprint, so the grooves
can be checked against the picture.

## Algorithms

### Medial axis (dc-geometry::medial)

Port of `medialAxis.ts` and `resolution.ts`. Inputs: a `Region` (outer
CCW, holes CW, ADR-0001), the flatten tolerance; output a `MedialGraph`.

```rust
pub struct MedialNode { pub p: Point, pub clearance_mm: f64 }
pub struct MedialGraph { pub nodes: Vec<MedialNode>, pub adjacency: Vec<Vec<usize>> }  // sorted neighbours
pub struct Resolution { pub step_mm: f64, pub budget_floor_mm: f64, pub budget_limited: bool }
pub fn resolution(region: &Region) -> Option<Resolution>;
pub fn medial_axis(region: &Region, step_mm: f64, tolerance_mm: f64) -> MedialGraph;
pub fn convex_corners(region: &Region, min_turn_rad: f64) -> Vec<Corner>;
pub fn point_in_loops(p: Point, loops: &[&Polyline]) -> bool;   // even-odd
```

1. **Resolution** (`resolution.ts`): `step = max(short_span / 160,
   perimeter / 40 000)` where `short_span` is the smaller side of the
   outer's bounding box and `perimeter` the sum of every loop's length;
   `None` when the span is 0. `budget_limited` when the second term won.
   Scale-invariant by construction (the ported test
   `resolution_scales_with_short_span`).
2. **Loops**: the region's outer and holes, arcs flattened at
   `tolerance_mm`, consecutive duplicates and an explicit closing vertex
   dropped (`cleanLoop`). A loop with fewer than three vertices is
   dropped; an outer with fewer gives an empty graph.
3. **Sampling** (`resampleLoop`): every edge split into
   `ceil(len / step)` equal pieces, originals kept. Samples are deduped
   by rounding to `1e-7` mm (integer pair key); the segments keep every
   piece.
4. **Delaunay**: `delaunator::triangulate` on the samples. Fewer than
   three samples, or a failed triangulation (all collinear), gives an
   empty graph.
5. **Nodes**: each triangle's circumcentre (the port of `circumcenter`,
   degenerate when `|d| <= 1e-9 · max(bl, cl)`) that lies inside the
   region by the even-odd test over all loops. Nodes are deduped by the
   same rounded key.
6. **Clearance and contact spread**: exact distance to the nearest
   boundary segment through a uniform grid (`SegmentDistanceGrid`, cell
   `2 · step`, bounded to 512 cells a side) with ring-by-ring expansion
   that stops when `best <= (ring − 1) · cell`. A node is kept when its
   clearance is at least `step · 1e-3` and the boundary within
   `clearance · (1 + tolRatio)` has a second contact at least 60° from
   the nearest one (`hasContactSpread`), `tolRatio = 0.003 +
   (step / clearance)² / 8`.
7. **Edges**: for each Delaunay half-edge pair whose two triangles both
   map to kept nodes, an edge, unless the edge's midpoint is outside the
   region (a reflex-corner cut).
8. **Contraction** (`contractMicroClusters`): nodes in the same
   `0.75 · step` grid cell merge into the member with the largest
   clearance.
9. **Spur pruning** (`pruneSpurs`): up to 32 passes, a leaf whose edge to
   its junction is shorter than `1.5 · step` and whose clearance is not
   below the junction's by more than `0.25 · len` is removed.
10. **Small components** (`dropSmallComponents`): components with total
    length under `4 · step` are dropped, unless none is longer, in which
    case the one holding the deepest node stays.
11. **Corner tips** (`regionConvexCorners`, `extendIntoCorners`): convex
    corners of the region (outer vertices turning into the region,
    island vertices the region wraps round) sharper than 50° get a
    zero-clearance tip node joined to the best candidate node: aligned
    with the bisector within `max(1.5 · step, 0.1 · projection)`, else
    one whose medial disc reaches the corner, scored as upstream, and
    only when the joining segment stays inside the region at up to 64
    samples.
12. **Finalise**: nodes with no edges are dropped; a graph with no edges
    keeps its deepest node alone; a graph whose extent is at most
    `3 · step` collapses to its deepest node (a dot). Adjacency lists are
    sorted.

The whole thing is pure and deterministic: grids are `Vec`s, keys are
integers, iteration is by index. Winding does not matter to any step:
`convex_corners` uses `cross · area`, as upstream, so it reads either
orientation; DreamCarve's regions are normalised anyway.

### V-Carve generator (dc-cam::vcarve)

1. **Tool.** `ToolKind::VBit { included_angle_deg: α }`, else
   `VCarveNeedsVBit`; α in (0, 180), else `VBitAngleRange`. `slope =
   tan(α/2)`; `cone = diameter / (2 · slope)`. Rates as every generator
   (feed, plunge; `rpm` checked at export).
2. **Carve.** Targets' closed profiles in project space, open ones
   warned `VCarveTargetOpen` and skipped; `nest` into regions (even-odd);
   crossing outlines are the `nest` error, as for Pocket. `start_plane =
   thickness − start_depth_mm` (`start_depth_mm` 0 or more, finite, and
   at most the thickness, else `CamError::InvalidStartDepth`). The floor
   depth `F = flat_depth_mm`.
3. **Cone check.** With `F`: `F <= cone + EPS` else `VBitConeExceeded`
   with `depth = F`. Without: after the medial axis is built, the deepest
   clearance `c_max / slope <= cone + EPS`, else the same error naming
   the region's instance and profile and the depth. The offset strategy
   checks the same on its deepest ring. S1 holds the carve's deepest Z
   to the stock bottom minus the allowance; a carve below that is
   `CamError::DepthBelowBottom`, as a Pocket's would be.
4. **Regions in cut order.** Regions visited by `sequence` (nearest
   neighbour, containment first), as blocks of one shape each; within a
   region the chain order below is fixed by the generator (`group`).
5. **Medial strategy, per region.** `resolution(region)`, else
   `VCarveNoMedialAxis`; `VCarveSamplingBudget` once when budget-limited.
   `medial_axis(region, step)`; if empty, retry at `max(budget_floor,
   step / 2)` up to twice (`MAX_AUTO_REFINEMENTS`); still empty warns
   `VCarveNoMedialAxis` and skips. Then `toolpath.ts`'s pipeline:
   - **Chains** (`extractChains`): maximal degree-2 paths between
     junctions or leaves, pure cycles (closed, first node repeated),
     isolated nodes as plunge points.
   - **Redundant leaves** (`pruneRedundantLeafChains`, tolerance
     `0.75 · step`): a leaf chain whose every body node's groove is
     covered by a remaining node's groove within the tolerance is
     dropped; zero-clearance tips are exempt; at most four passes.
   - **Depth** per node: `d = min(clearance / slope, F)`; `z = start_plane
     − d`. The carved disc radius at the start plane is `d · slope`
     (the clamped value, never the raw clearance: upstream's gouge fix).
   - **Simplify** (`simplifyChain`, tolerance `0.25 · step`):
     Douglas-Peucker on XYZ with the Z error scaled by `slope`.
   - **Blocks**: one `CutBlock` per chain, `cuts` the chain's `Linear`
     moves at their own Z (the toolpath is 3D: `Move::to` carries Z; the
     block's `z_mm` is its deepest point and S13 O4 pass monotony is
     read per block as the block's deepest Z, the same reading tabs
     already need). `entry` the chain's first point, `descent` empty
     (a straight plunge from the entry clearance), `cleared` the union
     of the chain's carved discs (a capsule chain built like
     `footprint`, with the per-node radius), `frees: false`. Chain order
     within the region: greedy nearest end, continuing through a shared
     junction without a retract (the block's `entry` equals the previous
     `exit`, `descent_from_z_mm` the previous Z, as Engrave's alternate
     passes do), otherwise a stay-down link when the gap is at most
     `0.9 · min(disc(exit), disc(entry))` and the link lies in the
     previous block's `cleared` (which `sequence::link_inside_cleared`
     checks), else a retract.
6. **Flat floor.** With `F`, where a node's clearance exceeds `F · slope`
   its depth is clamped (step 5), which is PureCutCNC's whole treatment.
   DreamCarve adds the floor (functional 39): `floor = floor_regions`
   (each region inset by `F · slope` with `offset`, round joins; islands
   grow by the same). For each floor region:
   - **Boundary trace**: the floor outline and each floor island as a
     closed `CutBlock` at `z = start_plane − F`, the tip exactly on the
     boundary so the flank meets the outline at the start plane. The
     medial chains clamped at `F` run inside it; the trace is what makes
     the wall a clean slope. Direction: as a Pocket wall pass, climb.
   - **V-bit floor**: the floor region minus what the clearing Pockets
     reach (step 7), cleared as `pocket::rings` clears a region with a
     tool of radius `F · slope` (the groove's half-width at the floor)
     and step-over `step_over_fraction · 2 · F · slope`, at `z =
     start_plane − F`, rings as `CutBlock`s with `cleared`. The floor is
     scalloped: between two grooves `s` apart the ridge is `s / (2 ·
     slope)` high. `check` prints that height (`V-bit floor ridges
     0.50 mm; a clearing Pocket or a smaller step_over_fraction
     flattens them`); the chapter recommends 0.1 for a V-bit's
     `step_over_fraction`. A region too thin for any ring is left to
     the medial chains (nothing to warn: the chains cover it).
   - Medial chains clamped at `F` that lie wholly inside the cleared
     floor (every node's clearance above `F · slope` and its disc inside
     the floor) are dropped: the floor pass cuts them. Chains that reach
     a corner or a narrow part stay.
7. **Clearing Pockets.** `cleared_by` lists, in project order, every
   enabled Pocket whose `clear_for` names this operation, with its tool
   radius. The V-bit floor region is `floor − ⋃ reach_i` where `reach_i`
   is what Pocket `i` can reach: its own region (see "Clearing Pocket")
   grown back by its radius, the same computation `pocket::rest` makes
   for `rest_of`. Never from a toolpath (S14).
8. **Offset strategy** (`vcarve.ts`): `step_over_mm` in (0, ∞), else
   `CamError::InvalidStepOver`. Ring `k` is each region inset by
   `k · step_over_mm` (`offset`, round joins), at `d_k = min(k ·
   step_over_mm / slope, F)`, `z = start_plane − d_k`, until the inset is
   empty (`inset_is_empty`) or `d_k` reaches `F` (the last ring at `F`
   is the floor trace, then the V-bit floor as step 6 if `F`). Each
   ring is a closed `CutBlock` oriented by `ring_is_ccw(wall,
   direction)` as Pocket's rings (re-derived for Y-up there), started at
   its point nearest the previous ring's end (`rings::nearest`),
   `cleared` the ring's capsule at radius `d_k · slope`. Links between
   rings of one region stay down when the link lies in `cleared`
   (upstream's cross-Z link within 8 × stepover; here the `cleared` test
   decides and the budget is `sequence`'s), else retract. No corner
   tips: the rings are round. `MAX_RINGS` as Pocket's.
9. **Sequence.** `SequenceInput` with `stepover_mm` 0 (links only inside
   `cleared`), containment pairs from `nest`, the operation's
   `machining_order` (one pass, so both orders cut the same; `pass_first`
   is accepted and equal). The descent to each entry is a straight plunge
   at the plunge rate from the entry clearance above the **stock top**
   (not the start plane: S2 unchanged), as `sequence` does for every
   plunge block.
10. **Warnings and S9/S17 on the toolpath.** `assert_safe` grows a cone
    test (`test_support::assert_cone_in_material`): every cutting move,
    sampled at `tolerance_mm` along it, has its disc of radius `(start_plane
    − z) · slope` inside the carve's regions (point inside, distance to
    every outline at least the radius minus `EPS_MM`). Every generator
    test runs it.

### Clearing Pocket (dc-cam::pocket, `clear_for`)

In `generate_pocket`, `reference_of` gains the `clear_for` branch:

1. The named operation must be an enabled V-Carve or Inlay earlier in
   `operations`, with a flat depth (an Inlay always has one: `depth_mm`
   for the female, `plug_depth_mm` for the male); else
   `ClearForInvalid`. `targets` must be empty and `depth_mm` `None`.
2. The Pocket's regions are the carve's **floor regions**
   (`vcarve::floor_regions` with the carve's tool angle: the Pocket
   borrows the V-bit's α from the carve's tool). Its depth is `start_depth
   + F`; its `pass_z` levels come from its own step-down to that depth
   (the end mill steps down; the V-bit does not). Its allowance is its
   own `allowance_mm` (usually 0: the floor boundary already stands the
   V-bit's inset off the outline; Corbin's `pocket_stock_to_leave` is
   that inset, which `floor_regions` has applied).
3. Earlier clearing Pockets of the same carve are its `rest_of` in all
   but name: the Pocket's region is `floor − ⋃ reach_j` for every earlier
   enabled clearing Pocket `j` of the same carve, by `pocket::rest`'s
   computation. A region that leaves nothing reachable warns
   `ClearForNoRoom` (as `RestOfClearsNothing`).
4. Everything else is a Pocket: strategy, wall pass, direction, entry,
   corner relief (allowed; the V-bit cannot reach a floor corner either,
   and a user who wants square floor corners asks for it), S9 against the
   floor regions, S13.

`order::check` sees a clearing Pocket as a Pocket (it cuts inside the
carve's targets before the carve, which is the intended order; it warns
nothing). The carve's `cleared_by` is computed by the carve generator
from the project, not handed over: both sides derive the same regions
from the same inputs.

### Start depth and `start_depth_mismatch` (dc-cam::order)

For every enabled V-Carve, Inlay, Photo V-Carve and countersink Drill
with its start depth `s`:

1. Its **footprint outlines**: a carve's target outlines (a male Inlay's
   mirrored surround outer), a Photo's footprint rectangle, a
   countersink's target circles.
2. The **containing Pocket**: the last enabled Pocket earlier in the list
   (not a clearing Pocket) whose regions contain every footprint outline
   (each outline's vertices inside a region, outside its islands, by
   `Region::contains_point`; an outline crossing a region boundary does
   not count).
3. With a containing Pocket of depth `d ≠ s` (within `EPS_MM`): warn
   `StartDepthMismatch (op, Some(pocket), s, Some(d))`. With none and
   `s > 0`: `StartDepthMismatch (op, None, s, None)`. With none and
   `s = 0`, or a match: quiet. A warning, as the functional spec says:
   the pocket may come from another program.

### Countersink (dc-cam::drill)

In `generate_drill`, `DrillCycle::Countersink { diameter_mm: D,
start_depth_mm: s }`:

1. Tool a V-bit with α in (0, 180), else `CountersinkNeedsVBit` /
   `VBitAngleRange`. `D > 0`, `D <= diameter + EPS` else
   `CountersinkMouthExceedsTool`. `depth = D / (2 · tan(α/2))`
   (`countersinkTipDepth`); `depth <= cone + EPS` holds by construction
   when `D <= diameter`, so the cone check is the mouth check (upstream's
   `maxCutDepth` tolerance note applies: compare with `1e-9`).
2. Targets as Drill (circle test); a circle whose diameter is at least
   `D − 1e-9` is skipped with `CountersinkMouthNotWiderThanHole`.
3. `top = thickness − s`; `bottom = top − depth`; S1 with the allowance.
   `retract_z = min(safe_z, top + max(retract_mm, 0))` as Drill (the
   retract is measured from the hole's top, which is the start plane).
   One block per hole as Simple: `Rapid` to `retract_z`, `Linear` to
   `bottom` at plunge, `Dwell` if `dwell_s > 0`, `Rapid` to safe Z.
4. `DrillHole::cycle = Expanded` always, so no `G8x` is written whatever
   `use_canned_cycles` says (S18).
5. Order, S15 and the rest as Drill. `drill_cutter_unusual` is not raised.

### Inlay (dc-cam::inlay)

Both sides are a `vcarve::Carve` plus checks; the generator is
`vcarve::blocks` and `sequence`.

1. **Values.** `depth_mm` D, `glue_gap_mm` G, `plug_depth_mm` P required
   (`MissingDepth`-style errors naming the field); `0 <= G < D` else
   `InlayGlueGap`; male: `margin_mm` M `> 0` else `InlayMargin`. Tool and
   cone checks as V-Carve; the cone check uses `D` (female) and `P`
   (male).
2. **Design regions.** Targets nested as V-Carve's (step 2).
3. **Female.** `Carve { regions: design, start_plane: thickness,
   flat_depth: Some(D), strategy: Medial, cleared_by: clearing Pockets
   naming this op }`. Identical to a V-Carve with `flat_depth_mm: D`,
   `start_depth_mm: 0`, which acceptance 56 checks move for move.
4. **Male.**
   - `male_mirror`: the joint bounding box of the targets' outlines in
     project space, centre line `x = cx`; `Transform::mirror_x_about(cx)`
     (`[−1, 0, 0, 1, 2cx, 0]`). Applied to every target outline;
     `Contour::new` renormalises winding (a mirror reverses it; a mirror
     negates bulges, which `Transform` already handles). `check` and
     `export` print `inlay_male_mirrored` with `cx`.
   - `surround`: `union(offset(outer_i, +M) for every design region's
     outer)` minus `design_mirrored` (every region, so a hole in the
     design is carved: `difference` with the design's regions, which
     subtracts their outers and adds back their holes). Tolerance the
     project's.
   - `Carve { regions: surround, start_plane: thickness − (D − G),
     flat_depth: Some(P), … }`. The surround's outer edge gets a V wall
     too (upstream VCarve users draw a boundary rectangle and get the
     same); harmless, and the floor trace runs round it.
   - `InlayPlugBaseThin` when `thickness − (D − G + P) < 6`.
5. **Fit property** (acceptance 57; `inlay::female_and_male_walls_mate`):
   for sample points `q` on a design outline and heights `h` in `[0, D −
   G]`: the female's uncut cross-section at depth `h` is the design inset
   by `h · slope`, and the male's at height `h` above its start plane (`z
   = start_plane + h`) is the mirrored design inset by the same, read
   from the two toolpaths' cone discs. The test builds both toolpaths,
   mirrors the male's back, and checks that the carved discs of each
   toolpath touch (within 0.02 mm) the same outline at the same inset.

### Bitmap import and storage (dc-import::bitmap, dc-project::bitmap)

1. **Decode.** PNG by the `png` crate (every colour type and bit depth;
   palette expanded, 16-bit taken by its high byte with `Image16Bit`);
   JPEG by `zune-jpeg` (baseline and progressive; CMYK to RGB by the
   decoder). Dimensions capped at `MAX_BITMAP_SIDE_PX` = 8192.
2. **Grayscale.** RGB to luminance `round(0.299 R + 0.587 G + 0.114 B)`
   (Rec. 601, what PIL's `convert("L")` and OpenCV use, so RasterCarve
   and DreamCarve read a colour fixture alike); alpha composited on white
   first (`c' = a · c + (255 − a) · 255`, per channel, then luminance),
   with `ImageAlphaFlattened`. Already-gray input is copied.
   `ImageColourConverted` when the source had colour.
3. **Encode.** `png` encoder, grayscale 8-bit, no interlace, filter
   `Sub`, compression `BITMAP_PNG_COMPRESSION` (the crate's `Default`
   level pinned by name), so the bytes are a function of the pixels.
   Base64 standard alphabet with padding (`base64` crate).
4. **Load.** `Bitmap::pixels` decodes the base64 and the PNG; a PNG that
   is not 8-bit grayscale, or whose size disagrees with
   `width_px`/`height_px`, is `LoadError::Bitmap { id, reason }`
   (validation reports the same as `bitmap_pixels_mismatch` /
   `bitmap_not_grayscale_8` with a location; `load` fails only when the
   base64 or PNG cannot be decoded at all).
5. **Placement.** Row 0 of the pixels is the picture's **top** row and
   sits at `y = y_mm + height_mm` (Y-up). Pixel `(i, j)` (column `i`,
   row `j`) covers `x ∈ [x_mm + i · w, x_mm + (i+1) · w]`, `y ∈ [y_mm +
   height_mm − (j+1) · h, y_mm + height_mm − j · h]`, `w = width_mm /
   width_px`, `h = height_mm / height_px` (equal within validation's
   aspect rule).

### Photo V-Carve (dc-cam::photo)

Designed from RasterCarve's documented behaviour (its `--help`, README
and statistics output) and VCarve's PhotoVCarve form. Nothing here is a
port.

1. **Settings.** Tool a V-bit, α, `slope`, `cone` as V-Carve. `max_depth
   = max_depth_mm` required, `0 < max_depth <= cone + EPS` else
   `VBitConeExceeded`; S1 on `start_depth + max_depth`. `θ =
   line_angle_deg` in `[0, 180)`, `f = line_spacing_fraction > 0`
   (`< 1` warns `PhotoLinesOverlap`), `sample_mm > 0`,
   `stepover_retract_mm > 0`, else `PhotoSetting`. The bitmap by id, else
   `PhotoBitmapUnknown`; its pixels decoded once.
2. **Groove geometry.** Full-depth groove width `W = 2 · max_depth ·
   slope`; spacing `s = f · W`. Direction `u = (cos θ, sin θ)`, normal
   `n = (−sin θ, cos θ)`. The family is `{ p : n · p = n · c + k · s }`,
   `k ∈ ℤ`, through the picture's **top-left** corner `c = (x_mm, y_mm +
   height_mm)` (changed from the functional spec's bottom-left in this
   design: RasterCarve lays its lines from the picture's origin, which is
   its top-left; using the same corner is what lets acceptance 60 compare
   groove for groove). Each line is clipped to the footprint rectangle
   (a segment, or nothing); lines are ordered by `k` from the one through
   `c` outward across the picture, and alternate direction: even `k` runs
   along `+u`, odd along `−u`. With `hatch`, a second family at `θ + 90°`
   follows the first, ordered and alternated the same way.
3. **Samples.** Along a clipped groove from its start, points every
   `sample_mm`, plus the end point; at each the depth `d = (1 − g/255) ·
   max_depth` where `g = photo::sample` (bilinear between the four
   nearest pixel centres, clamped to the edge pixels), or `(g/255) ·
   max_depth` with `invert`. The point's `z = start_plane − d`.
   Consecutive samples whose XYZ are collinear within `EPS_MM` are merged
   into one move (a flat white stretch is one `G1`).
4. **Blocks.** One `CutBlock` per groove: `entry` the first sample's XY,
   `descent` empty (straight plunge at the plunge rate to the first
   sample's Z), `cuts` the samples as `Linear` moves at their own Z, `exit`
   the last sample. Between grooves: a `Linear` rise to `start_plane +
   stepover_retract_mm` at the feed rate, a `Linear` move at that height
   to the next groove's start, then the plunge. The block's
   `descent_from_z_mm` is the retract height and `sequence` is told
   (`SequenceInput::stay_down_links: Some(height)`) to link consecutive
   blocks of one group at that height with feed moves rather than retract
   to safe Z (a new `SequenceInput` field, read only by Photo; every other
   generator leaves it `None` and is unchanged). The first groove and the
   last retract use safe Z as every operation does. Grooves of one family
   are one `group` so `sequence` keeps their order.
5. **S19 on the toolpath.** `assert_safe` checks every cutting point
   inside the footprint rectangle (within `EPS_MM`), `z` within
   `[start_plane − max_depth − EPS, start_plane + EPS]`, and every
   between-groove move a feed move. S7 widens grooves by `d · slope` at
   their depth; at the picture's edge a full-depth groove reaches `W/2`
   past the footprint, which is why `init` sizes the stock with the
   margin and S7 stays the rule that matters.
6. **Cost.** A 100 × 50 mm picture at 30°, 2 mm, `f` 1.1: `s` 1.18 mm,
   about 110 grooves of up to 115 mm, 460 samples each: 50 000 moves
   before merging. Fine.

### Safety pass (dc-gcode::material, S17–S19)

`material.rs` re-derives each operation's material from the project, as
it does for Pocket and Profile, and never from the toolpath.

- **S17, cone.** For a V-Carve, an Inlay side or a Photo V-Carve: `α`
  from the tool, `start_plane` from the operation, the regions from the
  project (targets nested; the male's surround by `inlay::surround` on
  the mirrored targets; a Photo's footprint rectangle). For every
  cutting move of the operation in the parsed program: at both ends and
  at samples every `MATERIAL_SAMPLE_MM` (1 mm) along it, `d = start_plane
  − z` must be in `[−tol, F_or_max + tol]` and the disc of radius `d ·
  slope` must lie inside a region: the point inside, the distance to
  every outline segment near it at least `d · slope − tol`, `tol` the
  program's rounding plus `EPS_MM`. For a Photo the disc test is replaced
  by S19's footprint test (a groove's width may cross the picture's
  edge; S7 bounds it). A failure is `ConeOffMaterial` with the overlap.
  Also: the tip never below `cone` (`d <= cone + tol`).
- **S18, countersink.** For a Drill whose cycle is a countersink: every
  cut (`G1` with Z motion) in its span is vertical, at a target circle's
  centre, from at most the retract plane to exactly `start_plane − D /
  (2 · tan(α/2))` (within rounding); no `G73`/`G81`–`G83` word appears in
  the span; no other motion but rapids at or above the retract plane
  (S15) and `G4`. Else `CountersinkDepth`.
- **S19, grooves.** For a Photo: every cutting endpoint inside the
  footprint (within `tol`), `z` within the depth range; every XY move
  below safe Z that is not inside a groove (i.e. between a groove's last
  sample and the next groove's first) is a feed move at `start_plane +
  stepover_retract_mm` (within rounding), else `GrooveLinkRapid` /
  `GrooveOffPicture`. S2 already refuses a rapid there.

S6 reads a groove's merged samples as the straight cuts they are: the
toolpath holds the merged moves, so reproduction is move for move.

### Golden corpus

**PureCutCNC cases** (`tools/parity/cases/*.camj`, captured as phase 2's):

| Case | Operation | Fixture | Notes |
| --- | --- | --- | --- |
| `vcarve-medial-rect` | `v_carve_medial` | 40 × 10 mm rect, 60° V-bit, `maxCarveDepth` 5 | spine and corner tips; the 8.66 mm centreline is clamped |
| `vcarve-medial-square` | `v_carve_medial` | 20 × 20 mm, 90°, `maxCarveDepth` 3 (the clamp: DreamCarve `flat_depth_mm: 3`; no floor trace in PureCutCNC, so the floor trace and V-bit floor are compared as DreamCarve-only moves, README says so) | flat clamp |
| `vcarve-medial-circle` | `v_carve_medial` | 16 mm 64-gon, 60°, `maxCarveDepth` 5 | one plunge, clamped |
| `vcarve-medial-ring` | `v_carve_medial` | 40 mm square with 20 mm square island, 90°, `maxCarveDepth` 3 | island, cycle |
| `vcarve-medial-l` | `v_carve_medial` | the L of `testLShapeJunctionAndReflexCorner`, 60°, `maxCarveDepth` 5 | junction |
| `vcarve-medial-text-rag` | `v_carve_medial` | outline text "Rag", 10 mm, 60°, `maxCarveDepth` 5 | the resolver's polygons are also written to `tests/fixtures/vcarve/rag.json` for the ported tests; the font and its license are named in the README |
| `vcarve-medial-triangle` | `v_carve_medial` | (0,0) (24,0) (12,18), 60°, `maxCarveDepth` 5 | three arms; the 11.1 mm incentre is clamped |
| `vcarve-offset-square` | `v_carve` | 20 mm square, 60°, `stepover` 1, `maxCarveDepth` 4 | offset rings |

Every case clamps at or below the bench bits' cone heights (5.5 mm for
60°, 3.175 mm for 90°, both 6.35 mm), so the DreamCarve twin of each
case passes the cone check with `flat_depth_mm` equal to the clamp.
Compared by `golden.rs` as **in-order per region** with chains as sets
within a region (a chain's direction is the nearest-end choice, which
agrees, but junction continuation order can differ by a tie), cutting
moves within 0.02 mm in XYZ after Y mapping; the floor trace and V-bit
floor are DreamCarve-only and excluded per the README. The `.camj` V-bit
has `diameter` 6; the DreamCarve tool 6.35 mm.

**RasterCarve cases** (`tools/parity/photo/`): `capture.py`, run as
`python3 -I capture.py --rastercarve ../../../rastercarve --case
photo-raster-inch` with the RasterCarve checkout's own virtual
environment on the path. It runs `python -m rastercarve` with the case's
arguments on `tests/fixtures/photo/<picture>.png`, writes
`tests/golden/<case>/program.nc` as RasterCarve wrote it, and a README
with the command line, RasterCarve's commit, its statistics block, and
the frame mapping. Nothing else of RasterCarve's is read or written.

| Case | Picture | Arguments | DreamCarve |
| --- | --- | --- | --- |
| `photo-raster-inch` | `ramp.png` (64 × 32, black left to white right, linear) | `--width 0.1` (defaults: 30°, 0.08 in, 22.5°, 110 %, 0.01 in, safe 0.1, feeds 100/30) | 2.54 mm wide at the origin, `max_depth_mm` 2.032, `sample_mm` 0.254, the same bit and angle; the harness scales the golden by 25.4 |
| `photo-raster-metric` | `radial.png` (64 × 64, white centre to black edge, radial) | `--metric --width 100 -t 30 -d 2 -a 22.5 -s 110 -r 0.25 -z 5 -f 1000 -p 300` | as written |
| `photo-hatch` | `ramp.png` | as `photo-raster-inch` plus `--hatch` if the installed RasterCarve CLI has it, else the engine's `carve_strategy: hatch` through its Python API in a three-line driver that the README quotes | hatch |

Frame: RasterCarve's X is DreamCarve's X; RasterCarve's Y grows down the
picture from its top-left origin, so `y_dc = y_mm + height_mm − y_rc`.
Compared groove for groove in order, each sample within 0.01 mm XY and
0.05 mm Z after mapping and scaling; rapids, the lifts between grooves
and RasterCarve's final traverse are not compared (its lift height is a
different setting). Depth differs by at most `max_depth / 256` from
RasterCarve's quantisation plus its resampling; the smooth fixtures keep
the resampling difference under 0.01 mm (a 64-pixel ramp over 2.54 mm is
0.03 mm of depth per pixel).

## Module layout

```
crates/dc-geometry/src/{medial.rs (medial axis, resolution, convex corners, point_in_loops), medial/{grid.rs (SegmentDistanceGrid), tests.rs}}
crates/dc-project/src/{model.rs (+ VCarveOp, VCarveStrategy, InlayOp, InlaySide, PhotoVCarveOp, PhotoStrategy, Bitmap, DrillCycle::Countersink, PocketOp::clear_for, Project::bitmaps), bitmap.rs (Bitmap::pixels, Grayscale), ids.rs (+ BitmapId), migrate.rs (v15_to_v16), validate.rs (+ rules above), serialize.rs (png_base64 on one line)}
crates/dc-cam/src/{vcarve.rs (Carve, blocks, floor_regions, cone_height_mm), vcarve/{medial.rs (chains, pruning, simplify, blocks), offset.rs, floor.rs}, inlay.rs, photo.rs, drill.rs (+ countersink), pocket.rs (+ clear_for in reference_of), pocket/rest.rs (reach shared with clear_for), order.rs (+ start_depth_mismatch), sequence.rs (+ stay_down_links), test_support.rs (+ assert_cone_in_material), warning.rs, error.rs}
crates/dc-gcode/src/{material.rs (+ S17, S18, S19), error.rs (+ variants), safety.rs (dispatch)}
crates/dc-import/src/{bitmap.rs, bitmap/{png.rs, jpeg.rs, gray.rs}}
crates/dreamcarve/src/{init.rs (+ pictures), image.rs (image add/list), cli.rs (+ Image), export.rs (+ dispatch of the three kinds, the summaries), preview.rs (+ <image>)}
tools/parity/{cases/vcarve-*.camj, cases/vcarve-*.notes.md, photo/{capture.py, README.md}}
tests/golden/{vcarve-medial-rect, vcarve-medial-square, vcarve-medial-circle, vcarve-medial-ring, vcarve-medial-l, vcarve-medial-text-rag, vcarve-medial-triangle, vcarve-offset-square, photo-raster-inch, photo-raster-metric, photo-hatch}/
tests/fixtures/{sign.svg, star.svg, photo/{ramp.png, radial.png, gradient.png}, vcarve/{rag.json, g.json, ga-small.json, ga-large.json}}
crates/dc-project/tests/fixtures/dcv/v15/sample.dcv
```

New dependencies, all `[workspace.dependencies]` with
`default-features = false`:

- `delaunator` 1 (ISC): Delaunay triangulation, the Rust port of the
  library PureCutCNC uses, so the medial axis triangulates as upstream
  does. No dependencies. `dc-geometry`.
- `png` 0.18 (MIT/Apache-2.0): PNG decode and encode. Pulls `fdeflate`,
  `flate2`/`miniz_oxide` (MIT/Apache-2.0), `crc32fast`, `bitflags`.
  `dc-import` (decode any PNG, encode the stored one), `dc-project`
  (decode the stored one).
- `zune-jpeg` 0.5 (MIT/Apache-2.0/Zlib): JPEG decode, pure Rust, one
  dependency (`zune-core`). `dc-import`. The `image` crate was considered
  and not used: its default features pull a dozen codecs, and two small
  decoders cover PNG and JPEG.
- `base64` 0.23 (MIT/Apache-2.0): the stored PNG's text form. `dc-project`.

`NOTICE` entries as their licenses require (ISC needs the copyright line;
the others are already represented by their license files in the
third-party listing). `cargo deny` is the gate.

## Test plan

Names are `crate::module::test`. "(port)" marks a PureCutCNC test to port
with its upstream name; "(golden)" a corpus comparison. Every
emitted-motion test runs `assert_safe`, which now includes the cone test
for V operations.

**dc-geometry — medial**
- `medial::rectangle_spine_has_exact_clearances_and_corner_tips` (port: `testRectangleSpineAndCorners`: 40 × 10 at step 0.5: one component; every node's clearance equals the exact boundary distance within 1e-6; a spine node near (20, 5) with clearance 5 ± 0.02; exactly one zero-clearance node at each corner).
- `medial::square_diagonals_meet_at_centre` (port: `testSquareCenterJunction`: deepest node within 0.3 of (10, 10), clearance 10 ± 0.05, connected, four corner tips).
- `medial::circle_collapses_to_centre` (port: `testCircleCollapsesToCenter`: 64-gon radius 8 at step 0.4: every node within 1 of the centre with clearance above 6; deepest equals the apothem ± 0.05).
- `medial::ring_with_island_is_a_cycle_without_island_tips` (port: `testRingWithIsland`: connected, edges ≥ nodes, clearance ≤ 10·(2−√2), corridor node (20, 5) at 5 ± 0.02, outer tips only, every edge midpoint inside the ring).
- `medial::l_shape_has_a_junction_and_no_reflex_tip` (port: `testLShapeJunctionAndReflexCorner`).
- `medial::convex_corners_are_classified` (port: `testRegionConvexCorners`: L has 5, ring 4, 64-gon 0, notched island apex counts).
- `medial::acute_corner_tips_survive_contact_filtering` (port: `testAcuteCornerTipsSurviveContactFiltering`: 20° triangle at step 0.2 keeps 3 degree-1 tips).
- `medial::resolution_scales_with_short_span`, `medial::large_shape_keeps_scale_relative_resolution`, `medial::resolution_is_unit_equivalent`, `medial::degenerate_region_is_rejected`, `medial::sample_budget_overrides_shape_resolution` (ports of `resolution.test.ts`).
- `medial::scaled_rectangles_have_identical_topology` (port: `testAutomaticResolutionKeepsScaledTopologyStable`).
- `medial::scaled_outline_text_keeps_topology` (port: `testScaledOutlineTextTopologyStaysStable`, on `tests/fixtures/vcarve/rag.json`), `medial::lowercase_g_rejects_flattening_spokes_at_both_sizes` (port: `testOutlineGRejectsFlatteningSpokesAcrossScales`, on `g.json`: non-corner leaves ≤ 3, chains ≤ 18, equal chain counts).
- `medial::saved_glyph_project_rejects_noise_across_linked_scales`, `medial::fresh_quarter_inch_glyph_rejects_grid_corner_noise` (ports of `noiseRegression.test.ts` on `ga-small.json`, `ga-large.json`; the asserted counts are the README's captured values).
- `medial::dot_region_is_one_node` (the geometry half of `testDotRegionCollapsesToSinglePlunge`).
- `medial::winding_does_not_change_the_graph` (a CW outer gives the same nodes and edges as CCW; DreamCarve's Y-up note).
- `medial::same_region_twice_is_identical` (determinism).
- Property (`proptest`, small): `medial::every_node_is_inside_its_region`, `medial::every_clearance_equals_the_exact_distance` on random convex polygons.

**dc-project**
- `migrate::v15_project_gains_bitmaps_and_clear_for`, `migrate::v15_with_bitmaps_already_present_fails`, `migrate::version_15_fixture_loads`, `cli::v15_project_exports_the_same_program`.
- `serialize::v_carve_inlay_photo_round_trip`, `serialize::v_carve_strategy_is_string_or_object`, `serialize::countersink_cycle_round_trips`, `serialize::bitmap_png_is_one_line`, `bitmap::pixels_decode_the_stored_png`, `bitmap::wrong_size_png_is_an_error`, `bitmap::colour_png_is_an_error`.
- `validate::v_carve_with_end_mill_is_error`, `validate::v_bit_angle_out_of_range_is_error`, `validate::flat_depth_beyond_cone_is_error`, `validate::photo_max_depth_beyond_cone_is_error`, `validate::countersink_depth_beyond_cone_is_error`, `validate::clear_for_must_name_an_earlier_enabled_carve_with_a_flat_depth`, `validate::clear_for_pocket_must_have_no_targets_and_no_depth`, `validate::clear_for_and_rest_of_are_exclusive`, `validate::countersink_mouth_wider_than_tool_is_error`, `validate::negative_start_depth_is_error`, `validate::inlay_glue_gap_must_be_below_depth`, `validate::inlay_male_needs_margin`, `validate::thin_plug_base_warns`, `validate::photo_angle_and_spacing_ranges`, `validate::photo_spacing_below_one_warns`, `validate::photo_names_a_missing_bitmap`, `validate::bitmap_aspect_must_match_pixels`, `validate::countersink_drill_with_v_bit_is_not_unusual`.

**dc-cam — vcarve (medial)**
- `vcarve::square_clamps_at_flat_depth_and_surfaces_at_corners` (port: `testGeneratorSquare`: 20 × 20, 60°, flat 2: every cut inside the square, min Z −2 ± 1e-6, max Z ≥ −1e-6, no warnings, continuity).
- `vcarve::depth_follows_the_bit_angle` (port: `testGeneratorDepthMatchesVBitGeometry`: 30 × 5 bar, 60° → −4.330 ± 0.02, 90° → −2.5 ± 0.02).
- `vcarve::end_mill_is_refused` (port: `testGeneratorRequiresVBit`), `vcarve::same_input_twice_is_identical` (port: `testGeneratorDeterminism`), `vcarve::two_bars_are_never_bridged` (port: `testGeneratorMultipleFeatures`: no cut has both ends in the 10 mm gap), `vcarve::nearest_region_is_cut_first` (port: `testGeneratorVisitsNearestResolvedRegionFirst`), `vcarve::outline_text_target_carves` (port: `testGeneratorTextFeatureTarget`, `rag.json`), `vcarve::dot_region_is_one_plunge_and_no_cut` (port: `testDotRegionCollapsesToSinglePlunge`), `vcarve::nearby_chain_ends_link_tool_down_within_the_carved_disc` (port: `testEmissionLinksNearbyChainEnds`: one plunge and three cuts at clearance 1; two plunges at 0.05), `vcarve::links_use_the_clamped_disc` (port: `testLinkRespectsClampedDepth`), `vcarve::triangle_arms_surface_at_three_corners` (port: `testGeneratorTriangle`).
- `vcarve::rectangle_centreline_depth_is_half_width_over_tan` (acceptance 48: 20 × 5, 60° 6.35 mm: centreline −4.330 ± 0.01, every cutting point's depth equals its wall distance / tan 30° ± 0.02, each corner reached at Z ≥ −0.01), `vcarve::six_mm_square_with_90_degree_bit_meets_at_three_mm`, `vcarve::six_mm_circle_is_one_plunge_at_5_196`.
- `vcarve::ring_runs_along_its_centreline_and_rises_at_outer_corners` (acceptance 49: 24 mm square with 12 mm hole, 90°), `vcarve::no_cut_is_nearer_a_wall_than_its_depth_allows` (the cone test on every fixture), `vcarve::triangle_meets_at_the_incentre`.
- `vcarve::open_path_is_skipped_with_warning`, `vcarve::cone_exceeded_is_an_error_naming_the_depth` (acceptance 52), `vcarve::flat_depth_within_the_cone_exports`.
- `vcarve::start_depth_lowers_every_cut_and_plunges_from_the_stock_top` (acceptance 53), `vcarve::one_pass_whatever_the_depth`.
- `vcarve::chains_become_blocks_that_sequence_in_generator_order`, `vcarve::both_machining_orders_cut_the_same` (one pass).

**dc-cam — vcarve (flat floor and clearing)**
- `vcarve::floor_boundary_is_traced_at_the_flat_depth` (acceptance 50: 30 × 10, 90° 6.35 mm, flat 2: nothing below −2; the 26 × 6 boundary traced at −2 within 0.01; bisector runs from each floor corner to the surface corner).
- `vcarve::v_bit_floor_covers_the_floor_region` (union of 4 mm footprints at −2 covers the floor within 0.05 at `step_over_fraction` 0.4 and 0.1), `vcarve::floor_ridge_height_is_reported`.
- `vcarve::chains_inside_the_cleared_floor_are_dropped`, `vcarve::chains_reaching_corners_are_kept`.
- `pocket::clear_for_takes_the_floor_region_and_depth` (acceptance 51: footprint inside 26 × 6, depth 2), `vcarve::floor_cutting_shrinks_to_what_the_end_mill_cannot_reach` (within 1.5875 of the boundary; under a third of the no-Pocket length), `vcarve::floor_coverage_with_the_pocket_still_reaches_the_floor`, `pocket::second_clearing_pocket_clears_only_what_the_first_left`, `pocket::clear_for_after_the_carve_is_an_error`, `pocket::clear_for_without_a_flat_depth_is_an_error`, `pocket::clear_for_with_targets_is_an_error`, `vcarve::disabled_clearing_pocket_gives_the_v_bit_the_whole_floor`, `pocket::clear_for_that_reaches_nothing_warns`, `pocket::clear_for_footprint_stays_inside_the_floor` (S9/S14).

**dc-cam — vcarve (offset)**
- `vcarve::offset_rings_are_inset_by_their_depth` (acceptance 54: 6 mm square, 0.5 mm steps, 90°: insets 0.5…2.5 at Z −0.5…−2.5 ± 0.01), `vcarve::offset_climb_and_conventional_reverse_every_ring`, `vcarve::offset_stops_at_the_flat_depth_and_clears_the_floor`, `vcarve::offset_rings_link_tool_down_only_inside_the_carved_band`, `vcarve::offset_ring_direction_matches_pocket_rings` (Y-up re-derivation note).

**dc-cam — drill (countersink)**
- `drill::countersink_depth_is_mouth_over_two_tan_half_angle` (port: "90° V-bit: a 6 mm mouth plunges 3 mm" and "82° V-bit: depth follows D / (2·tan(θ/2))": −3 and −3.451), `drill::countersink_depth_is_measured_from_the_start_plane` (port: "depth is measured from each target's own top face", restated: start depth 4 → Z −7), `drill::countersink_is_a_centre_plunge_and_retract` (port: "motion is centre-plunge and retract"), `drill::countersink_needs_a_v_bit` (port), `drill::countersink_rejects_an_unusable_angle` (port), `drill::countersink_rejects_a_missing_or_zero_diameter` (port), `drill::countersink_mouth_wider_than_the_cutter_is_refused` (port), `drill::full_diameter_countersink_is_cut_within_one_ulp` (port: "a tool limited to exactly its cone height still cuts"), `drill::mouth_no_wider_than_the_hole_skips_that_target` (port), `drill::non_circular_targets_are_skipped` (port), `drill::countersink_records_no_canned_cycle` (port: "countersink posts as linear motion even on a canned-cycle machine", the generator half), `drill::other_cycles_are_unchanged_by_the_new_variant` (port: "legacy drilling operations are untouched"), `drill::countersink_dwell_is_at_the_bottom`.

**dc-cam — inlay**
- `inlay::female_equals_a_flat_depth_v_carve` (acceptance 56, move for move).
- `inlay::male_mirrors_about_the_targets_centre_line` (acceptance 57: the mirrored outlines; the note's X), `inlay::male_carves_the_surround_and_the_holes` (every cutting point in the band or the circle), `inlay::male_floor_is_at_depth_minus_gap_plus_plug` (nothing below −5), `inlay::male_flank_passes_through_the_outline_at_the_start_plane` (within 0.02), `inlay::female_and_male_walls_mate` (the fit property, `z` 0..3, within 0.02), `inlay::glue_gap_at_or_above_depth_is_an_error`, `inlay::thin_plug_base_warns_on_10_mm_stock_and_not_on_12`, `inlay::clearing_pocket_clears_the_male_floor`, `inlay::surround_of_a_star_with_a_hole_has_the_hole_as_a_region`, `inlay::mirror_negates_bulges_and_keeps_areas` (arcs survive the mirror).

**dc-cam — order**
- `order::start_depth_matching_an_earlier_pocket_is_quiet`, `order::start_depth_without_a_pocket_warns`, `order::start_depth_with_a_shallower_pocket_names_it`, `order::disabled_pocket_does_not_count`, `order::countersink_start_depth_is_checked_too`, `order::photo_footprint_is_checked_too`, `order::clearing_pockets_are_not_containing_pockets`.

**dc-import — bitmap**
- `bitmap::png_gray_8_reads_as_is`, `bitmap::png_rgb_converts_by_luminance_and_warns`, `bitmap::png_rgba_flattens_on_white`, `bitmap::png_16_bit_takes_the_high_byte`, `bitmap::png_palette_expands`, `bitmap::jpeg_reads_and_converts`, `bitmap::extension_and_magic_must_agree`, `bitmap::too_large_is_refused`, `bitmap::encode_is_deterministic`, `bitmap::encode_then_decode_round_trips`.

**dc-cam — photo**
- `photo::grooves_are_spaced_by_the_fraction_of_the_full_depth_width` (acceptance 59: 1.179 ± 0.001 at 30°, 2 mm, 1.1), `photo::grooves_run_at_the_line_angle_and_alternate`, `photo::one_groove_passes_through_the_top_left_corner`, `photo::zero_angle_grooves_are_parallel_to_x`, `photo::every_cut_is_inside_the_footprint`, `photo::black_is_max_depth_white_is_the_surface_and_grey_is_between` (`gradient.png` with its blocks: −2.000, 0.000 ± 0.01; 50 % ± 0.02), `photo::invert_swaps_black_and_white`, `photo::hatch_adds_a_second_family_at_ninety_degrees`, `photo::between_grooves_the_bit_rises_to_the_retract_and_feeds_across`, `photo::spacing_below_one_warns`, `photo::collinear_samples_merge_into_one_move`, `photo::sample_is_bilinear_and_clamped_at_the_edges`, `photo::top_row_is_at_the_highest_y`, `photo::max_depth_beyond_the_cone_is_an_error`, `photo::start_depth_lowers_the_grooves`, `photo::unknown_bitmap_is_an_error`, `photo::same_input_twice_is_identical`.

**dc-gcode — safety**
- `safety::cone_through_a_wall_fails_s17` (a hand-built toolpath whose disc crosses the outline), `safety::tip_below_the_flat_depth_fails_s17`, `safety::tip_below_the_cone_height_fails_s17`, `safety::generated_v_carves_pass_s17` (every vcarve fixture), `safety::generated_inlays_pass_s17`, `safety::male_surround_is_derived_from_the_mirrored_targets`.
- `safety::countersink_at_the_wrong_depth_fails_s18`, `safety::countersink_written_as_g81_fails_s18`, `safety::generated_countersinks_pass_s18`.
- `safety::groove_outside_the_picture_fails_s19`, `safety::groove_link_as_a_rapid_fails_s19`, `safety::groove_below_max_depth_fails_s19`, `safety::generated_photos_pass_s19`, `safety::edge_groove_width_is_s7s_business` (a full-depth groove at the picture's edge passes S19 and fails S7 only when it leaves the stock).
- `safety::s17_check_is_fast_on_the_sign` (the sign fixture's two carves under 2 s, as `s9_check_is_fast_*`).

**dreamcarve (integration, `tests/`)**
- `cli::init_on_a_picture_writes_a_bitmap_and_a_photo_v_carve` (acceptance 58: `--width 100` → 100 × 50; `--height 50` the same; both or neither exit 2), `cli::image_add_places_a_picture_and_image_list_shows_it`, `cli::image_add_converts_colour_and_says_so`, `cli::moved_dcv_exports_the_same_program`, `cli::picture_past_the_stock_edge_fails_s7`.
- `cli::check_prints_the_carve_summary_and_the_mirror_note`, `cli::check_warns_start_depth_mismatch`, `cli::preview_draws_the_bitmap_under_the_grooves`.
- `cli::sign_exports_four_programs_in_tool_run_order` (acceptance 61: `fixtures/sign.svg`; every program passes S1–S19; the preview has the floor rings, the serif lines, two countersinks and the tabbed outline), `cli::star_female_and_male_export` (the inlay pair's two files from `fixtures/star.svg`), `cli::photo_quick_start_runs` (the chapter's commands).
- `quick_start::v_carved_sign_chapter_runs` (as the phase 2 plaque's).

**dc-gcode (integration, `tests/golden.rs`)**
- `golden::vcarve_medial_rect`, `golden::vcarve_medial_square`, `golden::vcarve_medial_circle`, `golden::vcarve_medial_ring`, `golden::vcarve_medial_l`, `golden::vcarve_medial_text_rag`, `golden::vcarve_medial_triangle` (in order per region, chains as sets, 0.02 mm), `golden::vcarve_offset_square` (rings as sets per level, 0.02 mm).
- `golden::photo_raster_inch`, `golden::photo_raster_metric`, `golden::photo_hatch` (groove for groove, 0.01 mm XY, 0.05 mm Z, after the frame map and, for the inch case, × 25.4).
- `golden::phase_1_and_2_cases_are_unchanged` (every earlier golden still passes: the `sequence` change for Photo links and the 3D block Z reading touch shared code).

## Slices

Ordered; each is one issue and one PR. Dependencies in brackets.

1. **Format v16: model, migration, validation.** Every new type and
   field; `v15_to_v16`; the validation rules; `Bitmap::pixels` with the
   `png` and `base64` dependencies; `BitmapId`. Tests: dc-project groups.
   [none]
2. **Medial axis geometry.** `dc-geometry::medial` on `delaunator`; the
   ported geometry and resolution tests; the text fixtures captured as
   polygon JSON by the parity tool (the capture change is part of this
   slice). Tests: dc-geometry medial group. [none]
3. **V-Carve, medial strategy.** `dc-cam::vcarve` with `Carve`, chains,
   pruning, simplification, blocks, the cone test in `assert_safe`, the
   start plane, `check`/`export` dispatch and summary. No flat floor yet
   (clamping only, as PureCutCNC). Tests: the vcarve (medial) group except
   the floor tests. [1, 2]
4. **Flat floor and the V-bit floor.** `floor_regions`, the boundary
   trace, the ridge rings, chain dropping. Tests: `vcarve::floor_*`,
   `vcarve::v_bit_floor_*`, `vcarve::chains_*`. [3]
5. **Clearing Pocket.** `clear_for` in `reference_of`, the reach
   subtraction on both sides, the warnings. Tests: `pocket::clear_for_*`,
   `vcarve::floor_cutting_shrinks_*`, `vcarve::floor_coverage_*`,
   `vcarve::disabled_clearing_*`. [4]
6. **V-Carve, offset strategy.** Tests: vcarve (offset) group. [3]
7. **Countersink.** Tests: drill (countersink) group. [1]
8. **S17 and S18 in the safety pass.** `material.rs` cone and countersink
   checks, the error variants. Tests: safety S17 and S18 tests. [3, 7]
9. **Inlay.** `dc-cam::inlay`, `Transform::mirror_x_about`, the note,
   the plug-base warning. Tests: inlay group. [4, 5]
10. **Bitmap import and the CLI.** `dc-import::bitmap` with `png` and
    `zune-jpeg`; `init` on a picture; `image add`/`list`; the preview
    image. Tests: dc-import bitmap group, `cli::init_on_a_picture_*`,
    `cli::image_*`, `cli::moved_dcv_*`, `cli::preview_draws_*`. [1]
11. **Photo V-Carve.** `dc-cam::photo`, `SequenceInput::stay_down_links`,
    S19 in `assert_safe` and `material.rs`. Tests: photo group, safety
    S19 tests, `golden::phase_1_and_2_cases_are_unchanged`. [10]
12. **`start_depth_mismatch`.** `order::check` extension; the CLI prints
    it. Tests: order group, `cli::check_warns_start_depth_mismatch`.
    [3, 7, 11]
13. **Golden corpus.** The eight PureCutCNC cases with notes; the
    RasterCarve capture script and three cases; the in-order-per-region
    comparison mode. Tests: golden group. [3, 4, 6, 11]
14. **Test parts and quick start.** `fixtures/sign.svg`, `star.svg`; the
    sign end-to-end test, the inlay pair test, the photo quick start
    test. [5, 7, 9, 11]
15. **Docs.** The chapters and quick start in the functional spec's
    deliverables; `CONTEXT.md` terms (Flat Depth, Start Depth, Clearing
    Pocket, Inlay, Glue Gap, Plug Depth, Photo V-Carve, Bitmap,
    Countersink). [all]
16. **Real cuts and release.** Cut the sign, the inlay pair and the photo,
    record them, tag `v0.3.0`.

Slices 1, 2 and 7 have no dependencies; 10 needs only 1. Three agents
can start on 1, 2 and 10 at once.

## Open questions

None for the maintainer. Two points the design decided and the
functional spec now records (both edited in the same PR):

- Acceptance 59 and capability 47: the groove family passes through the
  picture's **top-left** corner, RasterCarve's origin, not the
  bottom-left, so the goldens compare groove for groove.
- S17 for a Photo V-Carve: the tip stays in the picture (S19) and the
  groove's width may cross the picture's edge by up to the full-depth
  half-width, which S7 keeps on the stock; the disc test applies to
  V-Carve and Inlay, whose outlines are walls.
