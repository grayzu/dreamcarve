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
a picture and an `image` command. The format moves through versions 20 to 26, one per sub-phase and three more for the Inlay's cut-out, band rounding and one-file form ("Sub-phases"); phase 2's last changes took 16 to 19 (changed in spec sync 1).

```
SVG / DXF ──dc-import──▶ FeatureDefinitions ─┐
PNG / JPEG ─dc-import::bitmap─▶ Bitmap ───────┼─▶ Project (.dcv v20–v26, dc-project)
user edits .dcv ─────────────────────────────┘        │
                                                      ▼
   dc-cam::{profile, pocket, drill (+ countersink), engrave,
            vcarve (medial | offset, + clearing runs), inlay, photo}::generate
                         │  Vec<ToolRun> (one toolpath per tool) + Vec<Warning>
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
highest Y), and the male Inlay's mirror (about X, in place, on its male board).

Three things in this phase are **not** ports:

- **Clearing passes and the V-bit floor** (functional 39, 40) follow
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
| `dc-cam::vcarve::medial` | `vcarveMedial/toolpath.ts` (`extractChains`, `pruneRedundantLeafChains`, `simplifyChain`, `emitMedialToolpath`), `vcarveMedial/index.ts` (`generateVCarveMedialToolpath`: band loop, auto-refinement, nearest region first) | Chains become `CutBlock`s with an `entry_z_mm` and go through `sequence`; upstream's stay-down link rule (`LINK_CLEARANCE_FRACTION` 0.9 of the carved disc at the start plane) is **not** ported, because a link at depth across that disc runs below the final V surface (security review H2): a link stays down only when it passes the cone test at its own depth, else DreamCarve retracts. Bands are gone (one stock, no per-instance Z); the start plane replaces `band.topZ`. `simplifyTolerance` 0.25 × resolution and `redundancyTolerance` 0.75 × resolution kept. The flat floor trace and the V-bit floor are new (VCarve). |
| `dc-cam::vcarve::offset` | `src/engine/toolpaths/vcarve.ts` (`generateVCarveToolpathSingle`: `buildInsetRegions` rings at `currentDepth · slope`, `sortContoursNearestNeighbor`, the 8 × stepover cross-Z link budget) | Rings from `dc-geometry::offset` (cavalier_contours, round joins as upstream's `jtRound`). Ring direction re-derived as Pocket's `ring_is_ccw`. The cross-Z stay-down link is kept only where it passes the cone test at its depth (H2); the 8 × stepover budget is dropped. |
| `dc-cam::drill` countersink | `src/engine/toolpaths/drilling.ts` (`countersinkTipDepth`, `resolveCountersinkDepth`, `emitCountersinkPlunge`, the mouth-vs-hole skip), `drillingCountersink.test.ts` | Depth from the start plane (functional 44), not from a feature's own top face. The `maxCutDepth` check becomes the cone-height check from the tool's diameter (DreamCarve tools have no max cut depth). Never a `DrillHole` cycle record (stays `Expanded`). |
| `dc-cam::vcarve` tests | `vcarveMedial/vcarveMedial.test.ts` (22 tests), `resolution.test.ts` (5), `noiseRegression.test.ts` (2) | Ported; the text fixtures ("Rag", "g", the `gA` project) are captured as polygon JSON by the parity tool from PureCutCNC's resolver, since DreamCarve has no fonts until phase 5. |
| `tests/golden/vcarve-*` | captured via `tools/parity/` from `v_carve_medial` and `v_carve` operations | See "Golden corpus". |

Files in the first column that port logic carry the upstream attribution
header. `dc-cam::inlay`, `dc-cam::photo`, `dc-import::bitmap`,
`dc-project::bitmap` and the clearing code are DreamCarve's and
carry the DreamCarve header.

## Data model

### dc-project (format versions 20 to 26)

One version per sub-phase ("Sub-phases"): 20 adds the V-Carve kind with
its clearing list, 21 the countersink cycle, 22 the Inlay kind, 23 the
male Inlay's cut-out (added in 3.3 on 2026-10-30, #443), 24 the male
Inlay's outline rounding (added in 3.3 on 2026-10-31, #448), 25 the
one-file Inlay, whose `side` gives way to a `male_board` (added in 3.3
on 2026-11-02, #498), 26 `bitmaps` and the Photo V-Carve kind (24 until
#448, 25 until #498). Every step but 25 is additive, and a file from an
earlier version exports the same bytes. Step 25 replaces a field: a
female file exports the same programs, and a male file the same male
programs, both under the board's program names, but a male file now
also cuts the female ("Migrations").

```rust
pub const CURRENT_VERSION: u32 = 26;   // 20 in sub-phase 3.1, 21 in 3.2, 22, 23 (cut-out, #443), 24 (rounding, #448) and 25 (one-file Inlay, #498) in 3.3, 26 in 3.4
                                       // (phase 2 ended at 19: tool vendor, Drill hole width, Bore, stock to leave)

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
    /// same pixels give the same text within one build (a dependency
    /// bump may change it; no test compares the text).
    pub png_base64: String,
}
impl Bitmap {
    /// The PNG header alone: dimensions, colour type, bit depth. Cheap;
    /// what validation and `image list` read.
    pub fn header(&self) -> Result<BitmapHeader, LoadError>;
    /// The pixels, row-major from the top row of the picture, 0 black to
    /// 255 white. Checks the header against `width_px`, `height_px` and
    /// `MAX_BITMAP_SIDE_PX` before decoding any image data, decodes
    /// under an explicit `png::Limits` of `MAX_BITMAP_SIDE_PX² + one
    /// row` bytes with text chunks ignored, and computes the pixel count
    /// with `checked_mul` (security review M4). Decodes on each call (no
    /// cache in the model; `Clone` stays cheap and correct); only
    /// generation and the preview call it.
    pub fn pixels(&self) -> Result<Grayscale, LoadError>;
}
pub struct Grayscale { pub width_px: u32, pub height_px: u32, pub bytes: Vec<u8> }

pub enum Operation {
    Profile(ProfileOp), Pocket(PocketOp), Drill(DrillOp), Engrave(EngraveOp),
    /// Medial-line V-carving of closed shapes (functional 38–43).
    VCarve(VCarveOp),
    /// A V-bit inlay, both its boards (functional 45; one operation for
    /// both since format 25, #498).
    Inlay(InlayOp),
    /// A picture as parallel V-bit grooves (functional 47).
    PhotoVCarve(PhotoVCarveOp),
}

pub struct VCarveOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>,         // closed outlines; nested even-odd
    pub tool: ToolId,                     // must be a V-bit (validation, and the generator)
    pub strategy: VCarveStrategy,         // "medial" | { "offset": { "step_over_mm": …, "direction": … } }
    /// Clearing passes in cut order, largest tool first (functional 40).
    /// Empty by default: the V-bit clears its floor itself.
    pub clearing: Vec<ClearingPass>,
    /// Flat depth, mm below the start plane; `None` carves to full depth
    /// (functional 39; VCarve "Flat Depth").
    pub flat_depth_mm: Option<f64>,
    /// The start plane, mm below the stock top, 0 or more (functional 41).
    pub start_depth_mm: f64,
    /// The user vouches that another program has cleared the start plane
    /// over the targets (functional 41, changed after the security review
    /// of 2026-10-28). With `start_depth_mm > 0` and no enabled earlier
    /// Pocket containing the targets, `false` is the error
    /// `start_depth_uncleared`; `true` exports, with the flank engagement
    /// counted from the stock top (see "V-Carve generator", step 3).
    pub start_plane_cleared_externally: bool,
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

/// One clearing pass of a carve (functional 40): a flat end mill that
/// clears the carve's floor before the V-bit, as a Pocket run of its own
/// tool. Step-down and step-over come from the tool.
pub struct ClearingPass {
    pub tool: ToolId,                     // a flat end mill (validation `clearing_needs_end_mill`)
    pub strategy: ClearanceStrategy,      // Pocket's: "offset" | { "raster": { "angle_deg": … } }
    pub direction: CutDirection,
    pub entry: Entry,                     // Pocket's: "plunge" | { "ramp": … } | { "helix": … }
}

pub struct InlayOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub targets: Vec<InstanceId>,
    pub tool: ToolId,
    /// The board the male is cut from (format 25, #498; in `side`'s
    /// place). The project's stock is the female board.
    pub male_board: MaleBoard,
    pub depth_mm: Option<f64>,            // Corbin `inlay_depth`; None until set
    pub glue_gap_mm: Option<f64>,         // Corbin `inlay_fit_adjustment`
    pub plug_depth_mm: Option<f64>,       // Corbin `plug_depth`
    pub margin_mm: Option<f64>,           // Corbin plug stock offset; the male's, required once it is cut
    /// The male's (format 24, #448): the radius the band's outer edge is
    /// closed by ("Algorithms → Inlay", step 4); `None` by default and on
    /// migration, rounding by the edge pass's radius alone when the edge
    /// is open. Above 0 when set (`inlay_outline_rounding`).
    pub outline_rounding_mm: Option<f64>,
    pub clearing: Vec<ClearingPass>,      // functional 40; on both boards
    pub step_over_fraction: Option<f64>,  // V-bit floor, as VCarveOp
    pub machining_order: MachiningOrder,
    pub through_cut_allowance_mm: f64,
    pub safe_z_mm: f64,
    /// The male's (format 23, #443; Corbin's "Plug Cutout"): an end-mill
    /// pass that frees the plug, run last on the male board. `None` by
    /// default and on migration: the plug is sawn free. The female
    /// ignores it.
    pub cut_out: Option<InlayCutOut>,
}
impl InlayOp {
    /// The flank's engagement below `board`'s top: `D` on the female,
    /// `D − G + P` on the male; `None` until the values are set.
    pub fn engagement_mm(&self, board: Board) -> Option<f64>;
    /// Whether the male is cut: its board's thickness is set (#498).
    pub const fn cuts_male(&self) -> bool;
}

/// The board an Inlay's male is cut from (format 25, #498; "Algorithms →
/// Inlay", "Boards"). Saved right after `tool` as `"male_board": {
/// "width_mm": null, "height_mm": null, "thickness_mm": null }`.
pub struct MaleBoard {
    pub width_mm: Option<f64>,            // None: the stock's
    pub height_mm: Option<f64>,           // None: the stock's
    pub thickness_mm: Option<f64>,        // the male board's own; None: the male is not cut
}
impl MaleBoard {
    /// The male board as a `Stock`: its width and height, or the
    /// stock's where unset, and its own thickness.
    pub fn stock(&self, stock: &Stock) -> Stock;
}
impl Project {
    /// The project with its stock replaced by `inlay`'s male board
    /// (`MaleBoard::stock`), so the origin preset, heights and
    /// tolerances apply to the male board as to the stock. The male
    /// board shares the stock's bottom-left corner.
    pub fn on_male_board(&self, inlay: &InlayOp) -> Project;
    /// The first enabled Inlay that cuts its male, and its board.
    pub fn male_board(&self) -> Option<(&InlayOp, Stock)>;
}
/// The board a run or program cuts (#498): the stock (`Female`, every
/// operation's board and an Inlay's female's) or an Inlay's male board.
/// Not saved; `as_str` is `"female"` or `"male"`.
pub enum Board { Female, Male }
// Operation::board_run_tools(board): the tools an operation cuts with on
// one board (the stock: every operation's runs but an Inlay's cut-out;
// the male board: an Inlay's clearing passes, V-bit and cut-out, when
// it cuts its male). Operation::depth_mm() of an Inlay is the female's
// `depth_mm`, the deepest it cuts below the stock top.

/// An Inlay's cut-out (#443; "Algorithms → Inlay", step 6).
pub struct InlayCutOut {
    pub tool: ToolId,                     // a flat end mill; its step-down sets the passes
    pub tabs: Option<TabSpec>,            // the Profile's tab request; heights from the male board's bottom
    pub through_cut_allowance_mm: f64,    // S1 for the cut-out run, which goes through the male board
}

pub struct PhotoVCarveOp {
    pub id: OperationId, pub name: String, pub enabled: bool,
    pub image: BitmapId,
    pub tool: ToolId,                     // V-bit
    pub max_depth_mm: Option<f64>,        // VCarve "Max Carving Depth"; None until set
    pub start_depth_mm: f64,
    pub start_plane_cleared_externally: bool,   // as VCarveOp's
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

pub enum DrillCycle {
    Simple,
    Peck { peck_depth_mm: Option<f64>, retract: PeckRetract },
    /// A V-bit opens a mouth of `diameter_mm` at the start plane
    /// (functional 44). Saved as `{ "countersink": { "diameter_mm": 6.0,
    /// "start_depth_mm": 0.0, "start_plane_cleared_externally": false } }`.
    Countersink { diameter_mm: f64, start_depth_mm: f64, start_plane_cleared_externally: bool },
}

/// The largest picture a project may hold, per side, in pixels. Lives
/// here, not in `dc-import`, because a hand-edited `.dcv` reaches
/// `Bitmap::pixels` without passing an importer (security review M4).
pub const MAX_BITMAP_SIDE_PX: u32 = 8192;
```

Serialization: `VCarveStrategy` has a unit variant, so it is externally
tagged (`"medial"` or `{ "offset": { … } }`), like `DrillCycle`.
`PhotoStrategy` is a plain string; `MaleBoard` an object with all three
fields present, `null` when unset; `ClearingPass` is a struct with
every field present, as every persisted struct. `Operation` keeps its internal
`kind` tag: `"v_carve"`, `"inlay"`, `"photo_v_carve"`. `Bitmap::png_base64`
is the one long string in a `.dcv`; the formatter writes it on one line,
as it writes a vertex.

**Migrations.** `v19_to_v20`, `v20_to_v21` and `v21_to_v22` change no
field: each adds an operation kind (or a cycle variant) that a file from
before cannot contain, so they bump the version only. `v22_to_v23`
inserts `"cut_out": null` after `safe_z_mm` in every Inlay (#443),
`v23_to_v24` inserts `"outline_rounding_mm": null` after `margin_mm` in
every Inlay (#448), `v24_to_v25` replaces each Inlay's `side`, in its
place, with its `male_board` (#498), and `v25_to_v26` inserts
`"bitmaps": []` after `instances`. A file from any earlier version
exports the same bytes as the same project written at the current
version; a male Inlay with a fitting clearing pass from version 22 or 23
exports differently from the build that wrote it (#448 changed its band
edge before any release shipped the Inlay).

`v24_to_v25` gives a `"female"` Inlay an all-`null` male board, so the
file cuts what it did: the female, with `inlay_male_board_unset`. A
`"male"` Inlay gets the stock's width, height and thickness (thickness
`null` if the stock's was), and the project keeps its stock, so its male
programs are byte for byte the ones it wrote, under the male board's
names; it now also cuts the female on the stock. A side that is neither,
an Inlay that already has a `male_board`, or a male whose stock has no
width or height is refused. `dc_project::migrate_with_notes` returns,
for each such male of a file from version 22 to 24,
`MigrationNote::MaleInlayGainsFemale { operation, name }`, which
`Loaded::notes` keeps and `check` and `export` print as ``<file>: note:
operations[op1] `DC` was the male of a two-file inlay pair (format 24);
… Run only its male programs (export --board male), or make one project
for the pair from the design and set its male_board``.

Fixtures
`crates/dc-project/tests/fixtures/dcv/{v19,v20,v21,v22,v23,v24,v25}/sample.dcv`,
each holding the kinds its version has; `v24` is the previous version's
and `v25` the current one's until sub-phase 3.4 adds `v26`.

**Validation** (`validate.rs`), errors unless marked:

- `v_carve_needs_v_bit` (V-Carve, Inlay, Photo V-Carve, countersink Drill
  whose tool is not `ToolKind::VBit`; the Drill form is
  `countersink_needs_v_bit`). Located at `operations[i].tool`.
- `v_bit_angle_range`: the V-bit's `included_angle_deg` not in (0, 180),
  on every tool a V operation uses (the library already enforces it on
  import; a hand-edited project may not). Located at the tool's
  `cutter.v_bit.included_angle_deg`, and reported instead of the general
  `tool_out_of_range` for such a tool, so the angle is named once (#413).
- `operation_value_unset` extended to `flat_depth_mm`'s neighbours that are
  required: Inlay `depth_mm`, `glue_gap_mm`, `plug_depth_mm`, and
  `margin_mm` once the male is cut (its board's thickness set, #498);
  Photo `max_depth_mm`.
- `v_bit_cone_exceeded` (functional 39, 44, 47): a V-Carve or Inlay whose
  flat depth (or, with none, the deepest medial clearance: generator-time
  only, since validation has no geometry; validation checks only the
  flat depth and the Photo `max_depth_mm` and the countersink's derived
  depth) is deeper than `diameter_mm / (2 · tan(α/2))`. The engagement
  sum applies: the start depth counts too unless an enabled earlier
  Pocket at that depth contains the targets, so with no flat depth a start
  depth alone beyond the cone is already reported (located at
  `start_depth_mm`; with a flat depth at `flat_depth_mm`; #413). The
  generator repeats the check on the actual depth (see "V-Carve
  generator").
- `clearing_needs_flat_depth` (a non-empty `clearing` on a V-Carve with
  `flat_depth_mm: null`; an Inlay always has a flat depth),
  `clearing_needs_end_mill` (a pass's tool is not `EndMill`, as
  `cutter_not_flat` for a Pocket), `clearing_dangling_tool` (a pass names
  no tool of the project), `clearing_not_descending` (warning: a pass's
  tool is wider than the one before it), and a pass's `entry` angles by
  the Pocket rules. A pass has no corner relief, so the draft's
  `clear_for_with_relief` (security review L2) is impossible by
  construction.
- `start_depth_uncleared` (error; security review M2, decided
  2026-10-28): a V-Carve, Photo V-Carve or countersink Drill with
  `start_depth_mm > 0`, no enabled earlier Pocket whose regions contain its footprint outlines
  (nor, #426, an enabled earlier V-Carve whose flat floor does), and
  `start_plane_cleared_externally: false`. Located at `start_depth_mm`.
  Validation and `order::check` share one containment function,
  `dc_project::cleared_start_plane` (in `dc-project`, not `dc-cam::order`,
  because validation may not depend on `dc-cam`; decided in #413; named
  `containing_pocket` until #426);
  validation reports it so `check` names it with a location, and the
  generator refuses it too.
- `flatten_tolerance_range`: `flatten_tolerance_mm` above
  `MAX_FLATTEN_TOLERANCE_MM` (0.1 mm) is an error (security review L6:
  the S9/S17 slack and the medial axis's hole chords both scale with
  it).
- `countersink_mouth_exceeds_tool` (`diameter_mm > tool.diameter_mm`
  by more than `COUNTERSINK_TOLERANCE_MM`, upstream's `1e-9`, the
  generator's comparison), `countersink_diameter_positive`,
  `start_depth_negative` (any `start_depth_mm` below 0, located at the
  field: a countersink's is `cycle.countersink.start_depth_mm`, where its
  `start_depth_uncleared` is located too, #425). A countersink's
  `v_bit_cone_exceeded` is located at `cycle.countersink.diameter_mm`;
  its depth is derived, so the Drill's `depth_mm` is not used, not
  required (`Operation::depth_mm()` is `None` for it) and not checked
  (#425).
- `inlay_glue_gap_not_below_depth` (`glue_gap_mm >= depth_mm`; at
  `glue_gap_mm`), `inlay_plug_base_thin` (warning: the male board's
  `thickness − (depth − gap + plug) < 6`,
  `dc_project::INLAY_MIN_PLUG_BASE_MM`; at `plug_depth_mm`; the male
  board's thickness since #498, never the stock's),
  `inlay_margin_positive` (`margin_mm <= 0` with the male cut; at
  `margin_mm`), `inlay_outline_rounding` (format 24, #448:
  `outline_rounding_mm` set and not a positive finite number; at
  `outline_rounding_mm`).
  An Inlay's `v_bit_cone_exceeded` is on `InlayOp::engagement_mm(board)`
  for each board it cuts: the female's `D` always, located at
  `depth_mm`, and once the male is cut the male's `D − G + P`, located
  at `plug_depth_mm` with `from_stock_top` (the male board's top), so
  with both boards cut both can be reported; each, and the plug base,
  is compared only when the values it uses passed their own checks.
  `Operation::depth_mm()` of an Inlay is the female's `depth_mm`, the
  deepest it cuts below the stock top (#434; the male's engagement until
  #498, when the male moved to its own board).
- The male board (format 25, #498): `inlay_male_board_size` (`width_mm`
  or `height_mm` set and not above 0; at
  `operations[i].male_board.width_mm` or `.height_mm`),
  `inlay_male_board_thickness` (`thickness_mm` set and not above 0; at
  `operations[i].male_board.thickness_mm`), `out_of_range` for a size
  above `MAX_COORDINATE_MM` (6 m, as a stock side), and
  `inlay_male_boards_differ` (a later enabled Inlay that cuts its male
  names a different male board, as `MaleBoard::stock` resolves it, from
  the first one's, `Project::male_board`; at `operations[i].male_board`):
  every male program of a project is cut on one male board. The male's
  own rules (its margin, its cone, its cut-out, its plug base) apply only
  once the male is cut; an unset thickness is the generator's warning
  `inlay_male_board_unset`, not a validation problem.
- The cut-out (format 23, #443), checked once the male is cut (#498;
  until then the female ignores it, and `inlay_cut_out_needs_male`,
  the error for a cut-out on a female side, is gone with `side`):
  `cutter_not_flat` for its tool (at `cut_out.tool`), `inlay_cut_out_no_room` (the floor ring narrower than
  the tool's diameter `d`: `M + EPS_MM < P·tan(α/2) + d` with an open
  band edge, `M + EPS_MM < 2·P·tan(α/2) + d` with a V wall, #448; at
  `margin_mm`, naming the margin needed,
  `MaleBand::cut_out_margin_needed_mm` of `dc_project::male_band`). Its tool is a used tool, so its
  rates and step-down are required; its tabs follow a Profile's tab
  request rules, its allowance is 0 or more, and an unknown tool is
  `dangling_tool`.
- `photo_line_angle_range` ([0, 180)), `photo_line_spacing_positive`,
  `photo_lines_overlap` (warning: `line_spacing_fraction < 1`),
  `photo_sample_positive`, `photo_retract_positive`,
  `photo_unknown_bitmap`, `bitmap_footprint_aspect` (`height_mm /
  width_mm` off the pixel aspect), `bitmap_pixels_mismatch` (PNG size
  differs from `width_px`/`height_px`), `bitmap_not_grayscale_8` (the PNG
  is not 8-bit grayscale: DreamCarve wrote it, so a hand edit is the
  only way), `duplicate_bitmap_id`, `bitmap_empty` (`width_px` or
  `height_px` 0), `bitmap_too_large` (a side above
  `MAX_BITMAP_SIDE_PX`), `bitmap_footprint_positive` (`width_mm` or
  `height_mm` not above 0), and the `MAX_COORDINATE_MM` bound on the
  footprint's far corner (`x_mm + width_mm`, `y_mm + height_mm`). The
  size, colour-type and bit-depth checks read the PNG header only
  (`Decoder::read_info`), never the image data, so validation of a
  shared file costs nothing per pixel (security review M4).
- `drill_cutter_unusual` does not fire for a countersink Drill (its tool
  is a V-bit on purpose).

### dc-cam

```rust
/// One tool's toolpath of an operation, in its board's project space.
/// Every operation produces one run except a carve with clearing passes,
/// which produces one per pass (that pass's tool, `Clearing(tool)`) and
/// then its own (`Main`), and an Inlay, which does that on each board it
/// cuts.
pub struct ToolRun { pub label: RunLabel, pub tool: ToolId, pub toolpath: Toolpath, pub board: Board }
pub enum RunLabel { Main, Clearing(ToolId), CutOut(ToolId) }   // CutOut: an Inlay's cut-out, its male's last run (#443)
pub fn generate_vcarve(project: &Project, op: &VCarveOp) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError>;
/// Both boards (#498): the female's runs on the stock, then the male's on
/// `project.on_male_board(op)`, each tagged with its `Board`. The male's
/// `inlay_parts_merged` and `inlay_hole_filled` are dropped (the female's
/// name the same parts where they are drawn), and so is any warning the
/// female already gave.
pub fn generate_inlay(project: &Project, op: &InlayOp) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError>;
/// One board: the female (clearing passes, then the V-bit) or the male
/// (clearing passes, the V-bit, the cut-out). The male with its board's
/// thickness unset has no runs and warns `inlay_male_board_unset`.
pub fn generate_inlay_board(project: &Project, op: &InlayOp, board: Board) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError>;
pub fn generate_photo(project: &Project, op: &PhotoVCarveOp) -> Result<(Toolpath, Vec<Warning>), CamError>;
// generate_drill gains the countersink path; the phase 2 generators are unchanged.

/// Phase 2's block, with one field added (security review H1): a chain
/// starts at a corner tip at the surface and ends 4 mm down; `z_mm` is
/// its deepest Z, and `sequence` used to plunge an empty descent straight
/// to `z_mm`.
pub struct CutBlock {
    // phase 2 fields unchanged, plus:
    /// Z where cutting begins at `entry`, project space. A straight
    /// plunge or a stay-down link into this block ends here, never at
    /// `z_mm`. `validate_block` requires `entry_z_mm >= z_mm − EPS_MM`
    /// and the first cut to start at `(entry, entry_z_mm)`. Phase 1 and 2
    /// generators set it equal to `z_mm` (their cuts are flat), which
    /// `golden::phase_1_and_2_cases_are_unchanged` pins.
    pub entry_z_mm: f64,
}

pub struct SequenceInput<'a> {
    // phase 2 fields unchanged, plus:
    /// Photo V-Carve only: consecutive blocks of one group are joined at
    /// this Z by feed moves (a rise, a move, a plunge) instead of a
    /// retract to safe Z. `None` for every other generator, which is
    /// unchanged.
    pub stay_down_links: Option<f64>,
}

pub mod vcarve {
    /// What a V-bit carve is, independent of which operation asked for
    /// it: V-Carve builds one from its targets, Inlay one per board.
    pub struct Carve<'a> {
        pub regions: Vec<Region>,          // carved area, even-odd nested, project space
        pub shapes: Vec<ShapeRef>,         // each region's outer shape, for its blocks and errors (#404)
        pub start_plane_z_mm: f64,         // thickness − start_depth
        /// Z the flank is engaged from: the stock top, or the start plane
        /// when an enabled earlier Pocket has cleared it (security review
        /// H4). The cone check uses `engaged_from_z_mm − deepest_z`.
        pub engaged_from_z_mm: f64,
        pub flat_depth_mm: Option<f64>,
        pub strategy: &'a VCarveStrategy,
        pub step_over_fraction: Option<f64>, // V-bit floor; None when neither the op nor the tool sets one (#404), `MissingStepOver` once a floor is to be cut (#405)
        pub cleared_by: Vec<Clearing>,     // the clearing passes' tools, in order (see "Clearing passes")
    }
    pub struct Clearing { pub radius_mm: f64 }
    /// Blocks for one carve with one V-bit; the generators sequence them.
    pub fn blocks(project: &Project, op: &OperationId, tool: &Tool, carve: &Carve, warnings: &mut Vec<Warning>) -> Result<Vec<CutBlock>, CamError>;
    pub fn cone_height_mm(tool: &Tool) -> Option<f64>;   // diameter / (2 tan(α/2)); None for a non-V-bit
    /// Generator-side S17 (test support and `assert_safe`): the exact
    /// test of "Safety pass", run on the toolpath.
    pub fn cone_in_material(toolpath: &Toolpath, regions: &[Region], start_plane_z_mm: f64, engaged_from_z_mm: f64, slope: f64, cone_mm: f64, tolerance_mm: f64) -> Result<(), ConeViolation>;
}

pub mod inlay {
    /// The male's mirror: X reflected about the targets' joint bounding
    /// box centre line, in project space.
    pub fn male_mirror(project: &Project, targets: &[InstanceId]) -> Result<Transform, CamError>;
    /// The V-bit floor's ridges on `board`, along its rings and at their
    /// corners (#522), for `check`'s summary.
    pub fn floor_ridges(project: &Project, op: &InlayOp, board: Board) -> Option<FloorRidges>;
}
```

The two region constructions both the generators and the safety pass
need live in `dc-geometry`, so that `dc-gcode` re-derives material
without calling a generator (`docs/rust-guidelines.md`, "Crate
boundaries"; security review M3):

```rust
// dc-geometry
/// A carve's floor: each region inset by `inset_mm` (`flat · tan(α/2)`),
/// holes grown by the same; empty when nothing is left.
pub fn floor_regions(regions: &[Region], inset_mm: f64, tolerance_mm: f64) -> Vec<Region>;
/// A male inlay's band (#448): the regions' outers offset out by
/// `margin_mm + rounding_mm`, united, and inset by `rounding_mm`, a
/// morphological closing; with `rounding_mm` 0 the union of the offsets.
pub fn rounded_band(regions: &[Region], margin_mm: f64, rounding_mm: f64, tolerance_mm: f64) -> Vec<Region>;
/// A male inlay's surround: the band minus the regions themselves (so a
/// hole in the design is part of the surround, with walls all round).
pub fn surround(regions: &[Region], margin_mm: f64, rounding_mm: f64, tolerance_mm: f64) -> Vec<Region>;
/// The surround's floor: `floor_regions`, or with an open edge (#448) the
/// band regions keep their outer edge and lose the design's outers grown
/// by `inset_mm`; design holes are inset all round.
pub fn surround_floor_regions(surround: &[Region], design: &[Region], inset_mm: f64, open_edge: bool, tolerance_mm: f64) -> Vec<Region>;
/// The cut-out path (#443, #448, #461): `regions` (the band's edge `B`)
/// inset as `floor_regions` insets them; the outer outlines.
pub fn inset_outers(regions: &[Region], inset_mm: f64, tolerance_mm: f64) -> Vec<Contour>;
/// Whether a surround region lies inside a design outer (a design hole).
pub fn is_design_hole(region: &Region, design: &[Region]) -> bool;
impl Transform { pub fn mirror_x_about(x_mm: f64) -> Self; }
```

```rust
// dc-cam, continued
pub mod photo {

    /// The groove family: unit direction, perpendicular spacing, and the
    /// clipped grooves in cut order with their sample points. Fails with
    /// `CamError::PhotoSetting` when the family would exceed
    /// `MAX_PHOTO_MOVES` (2 000 000 samples; security review L3), as
    /// `MAX_RINGS` bounds a Pocket.
    pub struct Grooves { pub direction: Vec2, pub spacing_mm: f64, pub lines: Vec<Vec<Point>> }
    pub fn grooves(footprint: Rect, angle_deg: f64, spacing_mm: f64, sample_mm: f64) -> Result<Grooves, CamError>;
    pub const MAX_PHOTO_MOVES: usize = 2_000_000;
    /// Brightness 0..=255 at a project point, bilinear between pixel
    /// centres, clamped at the edges. Indices come from `f64::floor`
    /// clamped into `0..width_px` and are read with `get`, never from
    /// subtraction on a `u32` (security review M4).
    pub fn sample(bitmap: &Bitmap, pixels: &Grayscale, p: Point) -> f64;
}
```

`mod photo` opens the second `dc-cam` block above; `Bitmap::pixels` and
`photo::sample` are the only readers of pixel data.

New `WarningCode`s (params in parentheses): `VCarveTargetOpen (op,
instance, instance_name, profile)`, `VCarveNoMedialAxis (op, instance,
instance_name, profile)` (a region so thin that no interior node survives
two refinements; upstream `vcarveNoMedialAxis`), `VCarveSamplingBudget
(op, resolution_mm)` (once per operation; upstream `vcarveSamplingBudget`),
`ClearingNoRoom (op, tool, tool_id, diameter_mm)`, `ClearingNotDescending (op, tool, diameter_mm, earlier_tool, earlier_diameter_mm)` (export drops it: validation's `clearing_not_descending` says it once, #406), `StartDepthMismatch
(op, cleared_by: OperationId, floor: FloorKind, start_depth_mm,
floor_depth_mm: Option<f64>)` (`FloorKind` is `Pocket` or `VCarve`, a
V-Carve's flat floor, #426; `cleared_by` is not an `Option`: with no
floor holding the footprint the case is the error
`start_depth_uncleared`, #409), `CountersinkMouthNotWiderThanHole (op, instance,
instance_name, profile, mouth_mm, hole_mm)`, `InlayMaleMirrored (op,
axis_x_mm)`, `InlayPlugBaseThin (op, base_mm)` (generator-time too, from
the measured thickness), `InlayBandEdgeVWall (op, margin_needed_mm)`
(#448: a male with clearing passes none of which can cut its band's edge;
the least margin, `max(P·s + d_min, (D − G + 2P)·s)`, that would let one),
`InlayMaleBoardUnset (op)` (#498: the male board has no thickness, so
only the female is cut; `Warning::field()` locates it at
`operations[i].male_board.thickness_mm`),
`InlayBandClipped (op, edges: Vec<StockEdge>, open, narrowest_mm)` (#495:
a male band that runs past the male board, clipped to it; the edges, whether the
edge is open, and the band left at its narrowest), `InlayPartsMerged (op,
first, second, x_mm, y_mm, gap_mm, merge_mm)` (#479: two parts of an
Inlay's design closer than the merge distance, cut as one shape; the point
as drawn, the male's mirrored back), `InlayHoleFilled (op, hole, x_mm,
y_mm, width_mm, merge_mm)` (#511: a hole of an Inlay's design narrower
than the merge distance everywhere, filled; the middle of its box as
drawn, its mean width), `PhotoLinesOverlap (op, fraction)`. New
`CamError`s: `VCarveNeedsVBit { op, tool, tool_id, cutter }`,
`CountersinkNeedsVBit { op, tool, tool_id, cutter }`,
`CountersinkDiameterPositive { op, diameter_mm }`,
`CountersinkConeExceeded { op, engaged_mm, cone_mm }` (a countersink's
form of `VBitConeExceeded`, which names a carved shape and advises a flat
depth; #425),
`VBitAngleRange { tool, tool_id, angle_deg }`, `VBitConeExceeded { op,
instance, profile, depth_mm, cone_mm }`, `ClearingInvalid { op, tool, reason: ClearingProblem }` (the validation reasons, re-checked: `NoFlatDepth`, `NotEndMill { cutter }`, `UnknownTool`), `CountersinkMouthExceedsTool
{ op, mouth_mm, tool_mm }`, `InlayGlueGap { op, glue_gap_mm, depth_mm }`,
`InlayMargin { op, margin_mm }`, `InlayTooNearEdge { op, edge, distance_mm,
needed_mm, stock_mm }` (#495: a male's design too near an edge of the
male board that clips its band; "Algorithms → Inlay", step 4), `PhotoBitmapUnknown { op, bitmap }`,
`PhotoSetting { op, field, value }`, `BitmapDecode { bitmap, reason }`,
`StartDepthUncleared { op, start_depth_mm }` (the generator's form of the
validation error, re-checked as every generator re-checks validation),
`ConeOffMaterial { op, shape, x_mm, y_mm, z_mm, overlap_mm }` (the
generator-side S17 result from `cone_in_material`, raised by the V
generators on their own output before `sequence` returns it: a library
crate never hands back a toolpath it has proven wrong), `ConeArcMove {
op, shape, x_mm, y_mm, z_mm }` (#463: an arc in a V toolpath, which that
test is not written for).

### dc-gcode

```rust
pub enum ExportError {
    // phase 2 variants, plus:
    /// S17: a V-bit's cone crosses a wall of its carved region below the
    /// start plane, its tip leaves the region or is below the flat depth,
    /// or a cut above the start plane is not a vertical descent over the
    /// carve. The point is the worst one, project space (#408).
    ConeOffMaterial { op: OperationId, line: usize, x_mm: f64, y_mm: f64, z_mm: f64, overlap_mm: f64 },
    /// S17: the flank is engaged deeper than the cone is tall, measured
    /// from the stock top (or from a start plane an enabled Pocket has
    /// cleared), so the shank cuts.
    ConeHeightExceeded { op: OperationId, line: usize, engaged_mm: f64, cone_mm: f64 },
    /// S18: a countersink plunge ends somewhere other than the derived
    /// depth at a target's centre, or the program writes it as a cycle.
    /// `expected_mm` is the derived bottom, `found_mm` where the line
    /// ends, project space (#427).
    CountersinkDepth { op: OperationId, line: usize, expected_mm: f64, found_mm: f64 },
    /// S19: a groove sample outside the picture, or below the maximum
    /// depth, or a between-groove move written as a rapid.
    GrooveOffPicture { op: OperationId, line: usize, x_mm: f64, y_mm: f64 },
    GrooveLinkRapid { op: OperationId, line: usize },
    // #498, the male board:
    /// A male-board run of an operation that is no Inlay, or of an Inlay
    /// whose male board has no thickness.
    NotOnMaleBoard { op: OperationId },
    /// Two Inlays' male runs on different male boards.
    MaleBoardsDiffer { op: OperationId, first: OperationId },
    /// `emit` asked for one program across the stock and a male board.
    BoardsMixed { op: OperationId },
}
```

`material.rs` gains the three checks (see "Safety pass"). `EmitOp`
gains `label: RunLabel`, so a clearing run is an `EmitOp` whose `tool`
is the pass's end mill and whose program comment and summary read
`op2 (clearing t1)`; `emit_programs` already groups consecutive
`EmitOp`s by tool into programs, so nothing else in the emitter changes
for it.

`EmitOp` and `Program` gain `board: Board` (#498). `emit_programs`
emits the stock's programs first, then the male board's, each board
split by tool runs as before (or one `M6` program per board): every
male run of every Inlay is on the one male board, in operation order,
and female runs keep their place in the operation list. A male board's
programs are emitted and safety-checked against
`project.on_male_board(inlay)`, so its origin, heights, extents and
checks are that board's ("Safety pass"). The emitter does not trust its
caller: a male-board run of a non-Inlay or of an Inlay whose male board
is unmeasured is `NotOnMaleBoard`, two Inlays on different male boards
`MaleBoardsDiffer`, and `emit` of one program across two boards
`BoardsMixed`. `Material::of_run` takes the run's board.
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
height_px, limit_px }` (either side above `dc_project::MAX_BITMAP_SIDE_PX`,
checked on the header before any image data is decoded, with
`png::Decoder::set_limits` and `zune_jpeg::DecoderOptions::set_max_width
/ set_max_height` set to the same bound so a lying header cannot
allocate past it), `ImageFormatMismatch { extension, found }`.

### CLI

```
dreamcarve init <svg|dxf|png|jpg|jpeg> [-o] [--post] [--units] [--width <mm> | --height <mm>] ...
dreamcarve image add <dcv> <png|jpg|jpeg> (--width <mm> | --height <mm>) [--at <x>,<y>] [--name <name>]
dreamcarve image list <dcv>
dreamcarve export <dcv> ... [--board female|male]
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
and once per Inlay that cuts its male the `inlay_male_mirrored` note
with the mirror line's X.

**Boards** (#498). A project with an enabled Inlay names every program
after its board, `<stem>-female-<k>-T<n>.<ext>` and
`<stem>-male-<k>-T<n>.<ext>`, `k` counting from 1 within each board,
always (one program per board, and in `M6` one-program mode, too); a
project with no Inlay names its programs as before. `export` prints the
programs grouped under `Female board, the stock, 110.000 mm × 80.000 mm
× 12.000 mm:` and `Male board, 110.000 mm × 80.000 mm × 12.000 mm:`,
the first program of each board saying "set work zero (X, Y and Z)";
the extents grouped per board with `Estimated female board time:` and
`Estimated male board time:` when that board has two programs or more,
then the overall total. `check` groups its `Program dc-female-1-T2.nc
(T2):` blocks under the same headings. The Inlay summary is one line per
board, ``Inlay op1 `DC` (female board): deepest cut 4.000 mm below the
stock top, a flat floor at depth_mm 4.000 mm; …`` and ``Inlay op1 `DC`
(male board): deepest cut 5.000 mm below the male board's top, the
plug's start plane 3.000 mm below the male board's top and …``; an
operation line of a male run ends in " on the male board", and a
male-board warning reads ``operation `op1` (male board): …``.
`export --board female|male` writes one board's programs only and is
not saved in the project (re-cutting the male after a glue-gap change);
`--board male` on a project with nothing on the male board fails with
"nothing is cut on the male board: no enabled Inlay cuts its male (set
its male_board.thickness_mm)". A migration note (`Loaded::notes`) is
printed by `check` and `export` as `<file>: note: …`.

`--preview` draws the male board, when one is cut, beside the stock:
to its right with a 10 mm gap, bottom edges level, with its own dashed
outline (class `male-board`), each board's origin mark (class `origin`,
at the XY work zero the origin preset gives on that board), the Inlays'
mirrored shapes (class `male-shape`) and the male runs on it (group
class `operation male`); their legend entries end in " (male board)".
Without a male board the preview is unchanged. `--preview` draws each bitmap as an `<image>` element under the
toolpaths, at its footprint with `preserveAspectRatio="none"` and the
preview's Y flip, so the grooves can be checked against the picture. The
data URI is re-encoded from the decoded PNG bytes, not pasted from the
file's string, and passed through `escape_xml` like every attribute, so
nothing untrusted reaches the SVG text (security review L5); the chapter
says a preview with pictures can be large.

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
pub fn medial_axis_with(region: &Region, step_mm: f64, tolerance_mm: f64, reach: CornerReach) -> MedialGraph;  // #452
pub enum CornerReach { Upstream, Every { outer: bool, floor_inset_mm: Option<f64> } }
pub fn convex_corners(region: &Region, min_turn_rad: f64, tolerance_mm: f64) -> Vec<Corner>;  // arcs flattened as for medial_axis (#402)
pub fn point_in_loops(p: Point, loops: &[&Polyline]) -> bool;   // even-odd
pub fn bottleneck_mm(graph: &MedialGraph) -> Option<f64>;  // #475
pub fn refined_step_mm(resolved: &Resolution, graph: &MedialGraph) -> Option<f64>;  // #475
pub fn neck_mm(region: &Region, step_mm: f64, tolerance_mm: f64) -> Option<f64>;  // #553
pub fn neck_step_mm(region: &Region, resolved: &Resolution, tolerance_mm: f64) -> Option<f64>;  // #553
```

1. **Resolution** (`resolution.ts`): `step = max(short_span / 160,
   perimeter / 40 000)` where `short_span` is the smaller side of the
   outer's bounding box and `perimeter` the sum of every loop's length;
   `None` when the span is 0. `budget_limited` when the second term won.
   Scale-invariant by construction (the ported test
   `resolution_scales_with_short_span`).
   **The narrowest feature (#475).** The shape step suits shapes whose
   narrowest part is a fair fraction of their span, as letters' strokes
   are; traced art can have parts a hundred times narrower than its
   region. A node is the circumcentre of samples a step apart and stands
   off the axis by about `step² / (8 · clearance)`: once the step passes
   the clearance, the circle through two samples on one wall cannot touch
   it, and the node lands near the nearer wall with the clearance it finds
   there. A 50 mm region's step, 0.3125 mm, so missed the axis round a
   spike's tip 0.2 mm from the wall, and the groove stopped short there
   (the plug stood 0.087 mm proud on the synthetic spike, 0.19 mm on the
   maintainer's traced eagle). `bottleneck_mm` is the graph's narrowest
   bottleneck: the least clearance of a node joined to two or more others,
   none nearer the boundary. Along a corner's branch the clearance falls
   to the tip, a leaf, so corners are not bottlenecks; a node whose
   clearance dips below all its neighbours' is where the region narrows
   between walls that do not meet there, or, sampled too coarsely to see
   such a place, a node off the axis close to a wall. `refined_step_mm`
   is the step to sample again at when the bottleneck is narrower than the
   step: its clearance, but no finer than the step over
   `MAX_FEATURE_REFINEMENT` (4) nor the budget floor; `None` otherwise.
   Still scale-invariant, and at most four times the samples
   (`medial::refined_step_resolves_a_spike_beside_a_wall`,
   `medial::refined_step_keeps_the_step_of_a_wide_shape`,
   `medial::refined_step_is_bounded`).
   **A neck the graph does not see (#553).** A channel much narrower than
   the step can have no node at all: the samples either side of it lie a
   step apart along each wall, so every triangle across it has its
   circumcentre outside it, and the graph has no bottleneck there either.
   A male Inlay's band round lettering is sampled every 0.33 mm; where two
   letters passed 0.055 mm apart (lettering `0x8f`, 30°), the male cut
   nothing across the channel and the plug stood its whole height, 3.59
   mm, proud. `neck_mm` reads the narrowest neck from the boundary itself:
   from each sample, the nearest stretch of boundary within two steps
   that faces it across the region (into the region from both walls) and
   is not the same corner, on another loop, or on the same loop more than
   eight times the gap apart along it (a wedge counts only when sharper
   than 14°); half that gap, when under the step. `neck_step_mm` refines
   for it as `refined_step_mm` does for a bottleneck, the neck's clearance
   but no finer than a quarter of the step nor the budget floor, and the
   carve samples again at the finer of the two
   (`medial::neck_mm_finds_a_curve_beside_an_edge`,
   `medial::neck_mm_ignores_corners_and_wide_shapes`,
   `inlay::male_of_a_curve_beside_an_edge_cuts_the_neck`).
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
   **Round a feature (#465).** The spread test also drops the axis round
   the tip of a thin wedge of material standing into a region (a drawn
   line's end, a claw): there the axis is the curve equidistant from the
   tip and the wall beyond it, both seen in one direction once the axis is
   a little way out (from about 0.6 mm clearance for the line art's
   0.375 mm wedge). That axis is the only one whose discs reach the wall
   beside the wedge, so the groove stopped short of it and the line art's
   plug stood 0.191 mm proud. With `CornerReach::Every` (step 11) a node
   is also kept when the boundary has a second contact within the band
   such that: the boundary between the two, the shorter way round, turns
   by at least 60° in all (at the vertices between them, and at either
   contact that is itself a vertex), and they lie on neither one edge nor
   two edges that meet; the one stands out from the other's wall by more
   than 0.01 mm (`r · (1 − cos θ)` for clearance `r` and spread `θ`); and
   they are not both corners (vertices turning more than 50°). Between
   two contacts of a spoke, on one gently turning curve, the boundary
   turns by no more than the spread, so spokes are still dropped; round a
   wedge it runs up the wedge's flank and round its tip. A corner is
   reached by every node round it, so a node between two corners adds
   nothing. `medial_axis` (upstream's reach) keeps upstream's test alone
   (`medial::axis_wraps_round_a_line_end`,
   `medial::feature_contacts_keep_no_flattening_spokes`).
   **Corners turning 50° to 60° (#484).** A sharp corner (step 11) whose
   walls turn no more than the 60° spread has their contacts closer than
   the spread all along its branch; the band only widens near the tip,
   where `tolRatio` grows, so the test kept a stub there, the tip joined
   the stub, and the branch from it to the rest of the axis was missing
   (a regular heptagon's axis was four nodes, and its female and male cut
   nothing between the stubs). With `CornerReach::Every` a node on such a
   corner's branch (as step 9's spared leaves: within a step of the
   bisector, its clearance `along · sin(θ/2)` within a step) is kept too
   (`inlay::heptagon_plug_seats_flush`).
7. **Edges**: for each Delaunay half-edge pair whose two triangles both
   map to kept nodes, an edge, unless the edge's midpoint is outside the
   region (a reflex-corner cut).
8. **Contraction** (`contractMicroClusters`): nodes in the same
   `0.75 · step` grid cell merge into the member with the largest
   clearance.
9. **Spur pruning** (`pruneSpurs`): up to 32 passes, a leaf whose edge to
   its junction is shorter than `1.5 · step` and whose clearance is not
   below the junction's by more than `0.25 · len` is removed.
   **Long dead ends (#511).** With `CornerReach::Every`, no node of a
   chain from a leaf to its first junction longer than `5 · step` is
   pruned: a spur is a node or two off a junction, and a longer dead end
   is axis. Upstream's passes unzipped such chains up to 32 nodes: the
   axis down a channel of a male's band between two serif feet, whose
   exit got no circumcentre (lettering `0x47`,
   `stress::lettering_male_cuts_the_channel_between_two_feet`); the axis
   into a wedge narrower than the step either side of a pinch, where a
   hole comes within 3 µm of the outline (`inlay::pinched_design_seats_flush`);
   and a strip's round end (step 11, below, puts that back too).
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
    **Corner reach (#452).** `medial_axis` is `medial_axis_with(…,
    CornerReach::Upstream)`, all of the above. Its passes can leave a
    convex corner with no branch: spur pruning (step 9) unzips the branch
    into a corner sharper than about 29°, whose clearance falls only
    `sin(θ/2)` per mm, under the `0.25` it asks; upstream negates a hole's
    bisector, so a hole's corner (step 11) seeks its joint through the
    hole; and a corner turning under 50° gets no tip, though the
    contact-spread filter drops the branch into any wider than 120°.
    Where the bit starts at the surface that leaves the corner blunt or
    a whole branch of the cut missing (the V between a raised "v"'s
    arms). `CornerReach::Every`, which every carve uses (#452 for the
    male Inlay, #456 for V-Carve and the female), reaches every convex
    corner of the holes, and of the outer when `outer`: a leaf on a sharp
    corner's own branch (within a step of the bisector, its clearance
    `along · sin(θ/2)` within a step) is not a spur; every bisector
    points into the region; any other convex corner no medial disc, nor
    the floor's trace `floor_inset_mm` in (a corner turning `τ` is within
    `d (1 / cos(τ/2) − 1)` of it), reaches within its slack
    `reach_slack_mm` gets a tip too (0.01 mm for a 60° bit or wider;
    #506: scaled below that as the wall tolerance is, since the cone turns
    a sliver `g` wide into `g / tan(α/2)` of height, 0.076 mm at 0.01 mm
    with the 15° bit),
    worst first, each tip's branch counted before the next is chosen
    (#456: a round stroke end the contact-spread filter cut the axis
    short of has every flattened vertex a few hundredths beyond the axis;
    the branch into the worst runs to the curve's centre and reaches the
    rest, so the end gets one tip, not one per vertex); and a tip more
    than a step from its joint gets a node every step on the way, at its
    own clearance. No other corner's tip is a joint (#463): upstream's
    scores can pick the tip of the next vertex along a curve, 0.12 mm off
    and "aligned" with the bisector, over the axis a step away, and the
    branch then runs along the wall, a V-bit tip on the outline all the
    way, which S17 refuses (`medial::no_corner_tip_is_joined_to_another`).
    Before such a corner gets a tip, a branch spur pruning (step 9) took
    off is put back when one of its discs reaches the corner within
    0.01 mm, node by node to the junction it was unlinked from (#484): a
    strip's axis keeps its clearance to a round end's centre, so pruning
    unzipped up to 32 nodes of it, and the end's flattened vertices then
    lay a millimetre beyond the axis and off every bisector a tip could
    follow; the branch put back runs to the end's centre
    (`inlay::slot_plug_seats_flush`).
    `convex_corners` returns bisectors into the region.
    `medial_axis` (upstream's reach) stays for the ported tests.
    **A lone node (#511).** A graph with no edge after the passes (a
    regular octagon: its corners turn 45°, under the 50° a sharp corner
    turns, and the contact-spread filter drops every spoke) is its deepest
    node alone; a tip may join that node, which no tip could before, so
    the octagon was carved by a plunge at its middle and its corners stood
    short (`inlay::octagon_plug_seats_flush`).
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
3. **Cone check.** The flank is engaged from `engaged_from_z_mm`: the
   stock top, unless `dc_project::cleared_start_plane` finds an enabled earlier
   Pocket whose regions contain every target outline, or an enabled
   earlier V-Carve whose flat floor does (#426), at depth
   `start_depth_mm` within `EPS_MM`, in which case the
   start plane (security review H4). With no such floor and
   `start_depth_mm > 0`, `start_plane_cleared_externally` must be `true`,
   else `StartDepthUncleared` (M2, decided 2026-10-28). The engagement
   `e = engaged_from_z_mm − deepest_z` must satisfy `e <= cone + EPS`:
   with `F`, `deepest_z = start_plane − F`; without, after the medial
   axis is built, `deepest_z = start_plane − c_max / slope`; the offset
   strategy uses its deepest ring. Else `VBitConeExceeded` naming the
   region's instance and profile, `e` and `cone`. So a 4 mm start depth
   over uncleared stock with a 90°, 6.35 mm bit (cone 3.175 mm) is
   refused whatever the carve's own depth, while the same carve in a
   4 mm Pocket is checked on its own depth alone. S1 holds the carve's
   deepest Z to the stock bottom minus the allowance; a carve below that
   is `CamError::DepthBelowBottom`, as a Pocket's would be.
4. **Regions in cut order.** Regions visited by `sequence` (nearest
   neighbour, containment first), as blocks of one shape each; within a
   region the chain order below is fixed by the generator (`group`).
5. **Medial strategy, per region.** `resolution(region)`, else
   `VCarveNoMedialAxis`; `VCarveSamplingBudget` once when budget-limited.
   `medial_axis(region, step)`; if empty, retry at `max(budget_floor,
   step / 2)` up to twice (`MAX_AUTO_REFINEMENTS`); still empty warns
   `VCarveNoMedialAxis` and skips. Not upstream (#475): where the graph
   has a bottleneck narrower than its step, it is built once more at
   `refined_step_mm` (Medial axis, step 1), and the pipeline below uses
   that step (`inlay::spike_in_a_large_region_seats_flush`). Of the
   corpus only "Rag"'s "g" refines (0.073 → 0.018 mm, a node at the tip
   of the tail's wedge); its program is 13% longer and still within the
   golden's tolerance. So it is, at the finer step, where the region has
   a neck narrower than its step that the graph has no node in
   (`neck_step_mm`, #553); the goldens', the sign's and the tutorial
   pair's programs are unchanged by it. Then `toolpath.ts`'s pipeline:
   - **Chains** (`extractChains`): maximal degree-2 paths between
     junctions or leaves, pure cycles (closed, first node repeated),
     isolated nodes as plunge points.
   - **Redundant leaves** (`pruneRedundantLeafChains`, tolerance
     `min(0.75 · step, 0.005 mm)`): a leaf chain whose every body node's
     groove is covered by a remaining node's groove within the tolerance
     is dropped; zero-clearance tips are exempt; at most four passes.
     Upstream's `0.75 · step` lets a dropped chain reach that far beyond
     the grooves left, so the groove stops up to that short of a wall: on
     a wide male band (step 0.2 mm and more) a short branch into a fillet
     went, and the plug's wall stood `0.75 · step / tan(α/2)` too high
     (#484, `inlay::fillet_in_a_wide_band_seats_flush`). The cap is the
     carved edge's (simplify, below). That edge allowance, 0.005 mm, is
     for a 60° bit or wider; a narrower bit scales it as the wall
     tolerance (#478, #506), since the cone turns it into
     `0.005 / tan(α/2)` of height: 0.038 mm with the 15° bit, which held
     line art `0x55`'s plug proud (`stress::line_art_seats_with_the_edge_allowance_scaled`).
     The cap, the simplify tolerance and the fit's off-axis tolerance
     below all take the scaled value.
   - **Depth** per node: `d = min(clearance / slope, F)`; `z = start_plane
     − d`. The carved disc radius at the start plane is `d · slope`
     (the clamped value, never the raw clearance: upstream's gouge fix).
   - **Simplify** (`simplifyChain`; measured on the carved edge since
     #457): Douglas-Peucker on the chain's nodes, tolerance
     `min(0.25 · step, 0.005 mm)`. A stretch `a`–`b` stands for the nodes
     between when, for each such node `p`, `slope · (low − z_p) ≤ tol`,
     `low` the lowest the stretch's cone reaches over `p` (closed form, as
     the heightfield's): the carved surface rises no faster than
     `1 / slope` per mm, so that bounds the shortfall over the whole of
     `p`'s cone, its walls included. The stretch's cone may also reach no
     wall past 0.1 µm (`Walls::least_room`). Else it is split at the worst
     node (for a wall, the node nearest the worst point), so every kept
     point is a node at its own depth. Upstream measures the error on the
     axis (XY distance to the chord, Z error times `slope`, `0.25 ·
     step`), which lets a chord run a quarter step off the axis; its cone,
     at its own depth, then touches the nearer wall and stops up to twice
     that short of the farther (0.11 mm on the tutorial "DC" female). The
     edge measure drops the nodes' sideways noise that upstream's keeps,
     so programs get shorter (the sign's V-bit program 5630 → 4141
     lines).
   - **Fit** (#404, not upstream): a stretch between two kept points
     whose cone reaches more than 0.1 µm past a wall (a reflex corner or a
     flattened curve between them; the dropped points) is split at its
     worst point, set to that point's own depth, at most 16 times. Each
     stretch is also read every 0.05 mm, at most 16 times more (#457):
     for each point, its nearest wall point `w` and the largest disc
     inside the walls tangent at `w` with its centre on the line from `w`
     through the point (`Walls::ball_radius_mm`, the ball-shrinking
     construction: per wall segment the least `|q − w|² / (2 (q − w)·u)`
     in closed form) give the medial axis point `m` there, its radius `r`
     its exact clearance. Where the stretch carves more than 0.005 mm
     short of `m`'s cone at its own depth (`slope · (low − z_m)`), it is
     split at `m`, which is on the axis, so it gouges nothing. Points past
     the flat depth are left to the floor. Medial nodes are a step apart
     and merged within three quarters of one, so the cut between two of
     them, above all into a junction where a corner's diagonal meets a
     spine, can run off the axis even unsimplified: on the "DC" female a
     serif end's junction node sat 0.048 mm along the spine from the true
     one, and the plug stood 0.064 mm proud there. A point on a wall, or
     outside it within the walls' slack, has no nearest-wall direction to
     read its axis along, so it is read along the wall's own normal into
     the region instead, and only within the stretch's length, the reach
     of a corner's axis from its walls (#554). Where the medial axis joins
     a corner's tip to a node beside one of its walls, the stretch between
     runs along that wall, and splitting it where the wall bends a little
     into the region puts a point on the wall at the start plane; the rest
     of the way to the tip lay on the wall, out of the fit's reach, and the
     margin below then stopped the groove that far short of the tip: the
     plugs stood 0.340 and 0.051 mm proud past a pointed line end (line
     art `0x63`) and an "e"'s slit (lettering `0x97`,
     `vcarve::chain_along_a_wall_into_a_corner_is_fitted_to_its_axis`). A
     disc that grows farther is another part's axis, which its own chains
     carve; reading it, the "Rag" golden's chain into a nearly straight
     tip detoured 1.2 mm to it and back. Where
     material may stand above the start plane (`engaged_from` above it;
     #452), a stretch is also read every 0.05 mm and split where it runs
     more than 0.01 mm shallower than a point's own depth there (a
     corner's straight fall to the floor, Douglas-Peucker's chord across
     a bend in depth), and the chains are then cut off where they run
     out onto the floor, which the floor's cutting covers. Every carve's
     medial axis is built with `CornerReach::Every` (#456; outer `false`
     for a male Inlay's band region, whose outer edge is not the plug's;
     `floor_inset_mm` `F · slope`), so every convex corner of its walls
     is cut crisp (`vcarve::raised_text_corners_are_crisp`,
     `vcarve::female_corners_reach_the_tip`).
     **The tip stays in the region (#463).** A stretch's tip path must
     stay inside its region within `tol = flatten tolerance + EPS_MM`
     (`Walls::slack_mm`), the allowance step 10's test gives the tip: the
     walls are the flattened chords, which near a concave arc lie up to
     the flatten tolerance beyond the true wall. A stretch between two
     medial nodes can cut across the tip of a line that ends inside the
     region (a thin wedge of material, narrower than the medial step, so
     the Delaunay dual's edge passes it by), and at the start plane, where
     the cone is a point, the room test above cannot see it: the room at
     a crossing is 0, not less, so the old fit split the stretch onto the
     walls either side of the wedge and kept the move across it at the
     stock top ("would cut 0.0000 mm past a wall"). First, before the
     cone, fitting finds where the stretch leaves the region by more than
     `tol` (`Walls::excursion`): the crossings it leaves and comes back
     by, and, when both are on one loop, the loop's way between them that
     closes the smaller polygon with the stretch (outside an outer, the
     bay it crosses; in a hole, either side), and on it the vertex
     farthest from the stretch's line. That vertex is gone round: the
     stretch is split at the centre of the largest disc inside the walls
     tangent at it, along its bisector into the region
     (`Walls::ball_radius_mm`), which is on the medial axis, at its own
     depth (the walls' split budget). Second, the axis split above skips
     a sample point outside the walls, whose nearest wall is behind it.
     Last, a fitted stretch that still leaves the region is not cut: the
     chain is split there and the tool lifts over it, as an open band
     edge splits a chain (`vcarve::line_ends_carve`,
     `vcarve::line_art_regions_carve`).
     **Clear of the walls as written (#469).** The program writes each
     number to its places, which can move a point by up to `w = quantum /
     √2` (half a unit in the last place on each axis: 0.0007 mm at three
     millimetre places, 0.0018 mm at four inch places). A tip path nearer
     a wall than `w` can then cross it, and S17 refuses a written move
     that meets an outline away from its ends. A chain from a sharp
     corner's tip runs half the corner's angle off each wall, so a start
     rounded into the material put a 2° to 5° corner's first move across
     a wall a few hundredths along; one beside a line end ran 0.3° off its
     flank. So after fitting, a stretch whose tip path comes within `w`
     of a wall keeps only its parts from where the path is `w` clear at
     either end, along the stretch at its own Z, and a stretch nearer only
     between its ends is not cut; a stay-down link must keep `w` clear
     too. A corner's groove therefore stops where its tip is `w` from both
     walls, `w / sin(θ/2)` short of a corner of angle `θ`, leaving a
     sliver no wider than `w`. The 5° wedge's point is reached within
     0.016 mm, and the acute notch's plug seats within 0.028 mm, where it
     was 0.003 mm. S17 is unchanged
     (`safety::sharp_corners_export_at_any_placement`). The path keeps
     `w` from the walls as drawn too, arcs as arcs, not only from the
     chords it is fitted against (#481): beside an arc that bows into the
     region a chord lies up to the flatten tolerance beyond the arc, and a
     chain into a crescent's horn kept `w` from the chord ran 0.0001 mm
     from the arc and, written, crossed it
     (`safety::arc_horns_export_at_any_placement`,
     `vcarve::tip_paths_keep_the_written_margin_from_drawn_arcs`).
     **Corner starts on the written grid (#499).** Above its start plane
     a male Inlay's plug keeps the wood a corner's groove stops short of:
     `w (1/sin(θ/2) − 1) / tan(α/2)` high, 0.124 mm with the 15° bit in
     the 5° notch. A point on the grid the program writes on (the machine
     origin, `OriginXy::point_mm`, then whole units of the last place) is
     written exactly, so where the margin cuts a chain's end back, the
     chain starts instead at the grid point `g` near the end that leaves
     the least wood there, `|end − g| − clearance(g)`, among those whose
     move to the margin's point stays inside as written: a fraction `t`
     along, the path keeps `t · w` from every wall, chords and arcs as
     drawn, since that point may move by `w` (`vcarve::written`). The
     move also passes the cone test, within the slack fitting holds the
     chains to (`FIT_SLACK_MM`, 0.0001 mm; #565), and an open band
     edge's. The margin's point lies on a fitted stretch, so its own cone
     can reach that far past a wall, and every move to it ends there:
     held to `EPS_MM`, a margin point 0.000025 mm over refused every grid
     point, and line art `0x63`'s plug stood 0.177 mm proud past a 3.8°
     spike (`vcarve::written`'s
     `chain_end_past_a_wall_within_the_fit_slack_starts_on_the_grid`).
     The written path is held to the walls exactly as before. No written
     move crosses a wall, and S17 is unchanged. The grid limits it: a
     5° corner is `q / 11` wide `q` from its tip, so the nearest grid
     point inside can be several units in. The notch seats within
     0.05 mm with every bench bit in a millimetre program, and with the
     60° and 90° bits in a four-place inch program; the 30° and 15° bits
     can still stand 0.08 and 0.16 mm proud there in inches
     (`inlay::notch_seats_flush_with_narrow_bits`,
     `vcarve::corner_groove_runs_on_to_the_written_grid`). Millimetres
     do not always reach either: the nearest grid point inside a 4.5°
     notch can be 0.008 mm from its tip, and line art `0x7a`'s plug stood
     0.068 mm proud with the 15° bit (#559, open: a decision between
     accepting the limit in millimetres, more places, or a crossing within
     a unit).
   - **Blocks** (changed in #404): one `CutBlock` per run of chains the
     tool cuts without lifting (chains continued through a shared node or
     joined by a stay-down link; the link is a cut of the block), else
     per chain as below: `cuts` the chains' `Linear`
     moves at their own Z (the toolpath is 3D: `Move::to` carries Z; the
     block's `z_mm` is its deepest point and S13 O4 pass monotony is
     read per block as the block's deepest Z, the same reading tabs
     already need). `entry` the chain's first point and `entry_z_mm` its
     Z (a corner tip's is the start plane; security review H1), `descent`
     empty: a straight plunge from the entry clearance above the stock
     top to `entry_z_mm`, at the plunge rate. `cleared` is `None`: a
     chain's carved volume is a cone's sweep, which the 2D
     `link_inside_cleared` test cannot represent (security review H2).
     `frees: false`. Chain order within the region: greedy nearest end,
     continuing through a shared junction without a retract (the block's
     `entry` equals the previous `exit`, `descent_from_z_mm` the previous
     Z, as Engrave's alternate passes do); otherwise a **stay-down link**
     only when the straight move from the previous `exit` at its Z to
     this `entry` at its Z passes the cone test at its own depth: at both
     ends and wherever the clearance field is minimal along it (the
     generator has the region's `SegmentDistanceGrid`), the point's
     clearance is at least `(start_plane − z(t)) · slope + EPS_MM`, with
     `z(t)` interpolated, and the move crosses no outline. Else a
     retract. PureCutCNC bridges on the disc at the start plane, which
     lets a link at depth run below the final V surface by up to
     `0.9 · d`; DreamCarve retracts there instead. Links are not golden-
     compared, so parity holds. The link is emitted by the generator as
     a cut inside the block (#404: a run of linked chains is one block,
     so `sequence` needs no new link rule and has no cone test). A
     region starts at its shallowest chain end (a corner tip where there
     is one): the sequencer, not the generator, orders regions, so
     upstream's "nearest the previous region" is not known here.
6. **Flat floor.** With `F`, where a node's clearance exceeds `F · slope`
   its depth is clamped (step 5), which is PureCutCNC's whole treatment.
   DreamCarve adds the floor (functional 39): `floor = floor_regions`
   (each region inset by `F · slope` with `offset`, round joins; islands
   grow by the same). #465: `cavalier_contours` can return such an inset
   as open pieces, where a traced convex curve (its points a little off
   any one circle) of radius about `F · slope` collapses to about a
   point; `offset` dropped them, and with them the region's whole floor,
   so nothing cut its walls farther than `F · slope` from the medial axis
   (the maintainer's eagle male stood 1.17 mm proud in a hole of the
   design). `offset` now takes such an offset again with finer
   tolerances (phase 1 design, `offset`;
   `inlay::traced_hole_plug_seats_flush`). #489: an island shaped like a
   C whose mouth is narrower than `2·F·slope` grows over the mouth, and
   `offset` gives it back as two loops of one winding, its outline and an
   inner one round the bay; each loop was cut away as island, so the bay
   lost its floor and kept its wood (a C-shaped plug stood 3 mm proud).
   A grown island's loops are nested, so the bay is a floor of its own;
   so is the plug's foot grown with an open band edge
   (`surround_floor_regions`). The male's cut-out (`inset_outers`) still
   fills such a bay, as it does not cut round a hole of `B`. For each
   floor region:
   - **Boundary trace**: the floor outline and each floor island as a
     closed `CutBlock` at `z = start_plane − F`, the tip exactly on the
     boundary so the flank meets the outline at the start plane. The
     medial chains clamped at `F` run inside it; the trace is what makes
     the wall a clean slope. Direction: as a Pocket wall pass, climb.
     **Its arcs within the bit's wall tolerance (#478).** An arc of the
     trace drawn as lines `e` inside it stops the flank `e` short of the
     wall, and the cone turns that into height at `1 / tan(α/2)`: with the
     flatten tolerance's 0.01 mm the bench's 15° bit left a star's female
     0.070 mm shallow round its circle, the 30° 0.035. The trace's arcs are
     drawn within `dc_geometry::wall_tolerance_mm`, the flatten tolerance
     times `min(1, tan(α/2) / tan 30°)`, so a wall stands no further off
     true up the flank than the 60° bit's, whatever the bit; a 60° or
     wider bit's trace is drawn as before. The rings inside the trace keep
     the flatten tolerance (`vcarve::narrow_bit_traces_an_arc_within_its_wall_tolerance`).
   - **V-bit floor**: the floor region minus what the clearing passes
     reach (step 7), cleared as `pocket::rings` clears a tool-centre
     region (the floor region is one, for a tool of radius `F · slope`,
     the groove's half-width at the floor) with step-over
     `step_over_fraction · 2 · F · slope`, at `z = start_plane − F`,
     rings as `CutBlock`s with `cleared` (each ring's node region,
     inside the floor, so `sequence` may link two rings tool-down at `F`
     inside the floor, which passes the cone test by construction).
     `step_over_fraction` is the operation's, else the tool's; with
     neither, a carve with a floor to cut is `MissingStepOver` (outside
     (0, 1], `InvalidStepOver`), and one with no floor needs none (#405).
     With no clearing passes the tree's root rings are the boundary
     trace above, cut last (`WallRings::Last`). Each innermost ring
     (a node with no children) gets one more ring half a step-over
     inside it (`residual_regions` at `s / 2`, upstream's
     `appendResidualCoreGuides`), since the rings alone can leave the
     middle up to `s` from a groove, a ridge twice the reported one
     (#405). A ring with children leaves the same where it is under two
     step-overs across and its inset is empty or noise there, though not
     elsewhere (a strip between two wider parts): it gets centre rings
     too (#484), round the pieces of it more than `0.6 · s` from its
     ring and from its children (`residual_regions` at `0.6 · s`, just
     over the `0.586 · s` a square corner of two rings leaves, so a
     square corner gets none), less the pieces no wider than `s` either
     way, which are the slivers at corners sharper than a right angle that
     the corner's medial chain cuts along its bisector. Before, a 0.88 mm
     strip of a female's floor kept a ridge `s / slope` high along its
     middle, which held the plug 0.3 mm up
     (`inlay::dumbbell_floor_seats_flush`); the tutorial's "DC" floor gets
     a few such strips (its female's V-bit program 1080 → 1173 lines).
     Arcs of the rings are flattened to the floor's side:
     chords for a counter-clockwise arc, lines tangent to a clockwise
     one (outside its circle), so no move comes nearer a wall than its
     arc (#405). The floor is scalloped: between two grooves `s` apart
     the ridge is `s / (2 · slope) = step_over_fraction · F` high. At a
     ring's convex corner of angle θ the next ring's corner is
     `s / sin(θ/2)` back along the bisector, and where the medial chain
     does not cut that bisector at `F` (#522: a curved floor, or a floor
     tip where the design narrows, has no medial nodes there) the ridge
     is higher. The **corner ridge** is `step_over_fraction · F /
     sin(θmin/2)` for the sharpest convex corner θmin of any ring of the
     floors' ring trees, capped at `2 · step_over_fraction · F`: each ring
     is the one outside it inset by `s` and a leaf's centre ring is `s/2`
     in, so no floor point is more than `s` from a ring. The sweep's
     worst measured peaks were 1.5 times the straight-run ridge. `check`
     prints both heights (`V-bit floor ridges 0.800 mm along the rings
     and up to 1.131 mm at their corners; a clearing pass or a smaller
     step_over_fraction flattens them`, `vcarve::floor_ridges`), the
     corner ridge only where it is higher; the Inlay chapter sets
     `glue_gap_mm` above the female's corner ridge. Covering the corners
     instead, so the straight-run ridge holds everywhere, is #527 (3.6).
     The chapter recommends
     0.1 for a V-bit's `step_over_fraction`. A floor region that is
     noise at the flatten tolerance (Pocket's `is_noise`), or a region
     too thin for any floor, is left to the medial chains (nothing to
     warn: the chains cover it).
   - Medial chains clamped at `F` that lie wholly inside the cleared
     floor are dropped (in `medial::runs`, after fitting): every fitted
     point at `F` and inside a floor region that is cut. Fitting has
     held each stretch at `F` to the cone test, so its clearance is at
     least `F · slope` all along, and the floor and the trace then cut
     everything the chain would (#405 reads the draft's "its disc
     inside the floor" so). Chains that reach a corner or a narrow part
     rise above `F` there and stay whole.
7. **Clearing passes.** `cleared_by` is the operation's `clearing` list
   in order, each with its tool's radius. The V-bit floor region is
   `floor − ⋃ reach_i` where `reach_i` is what pass `i` can reach: the
   floor region inset by its radius and grown back by it, the same
   computation `pocket::rest` makes for `rest_of` (`rest::left_by`, which
   takes the list of references). Never from a toolpath (S14). Per floor
   region (#406): only the passes whose end mill fits that region count, so
   a region no end mill fits is cut exactly as with no passes; each piece
   left is cleared as step 6 clears a floor region, its own outline
   included (`WallRings::Last`, since part of it is the edge against the
   reach, a whole step-over from the first ring otherwise) and its centre
   rings; then the boundary trace, the whole floor region's outline and
   islands as closed blocks, is cut last, `cleared` the floor region. When
   the passes reach everything only the trace is cut, and no step-over is
   needed. Medial chains are still dropped against the whole floor (step
   6); a corner's chain keeps its stretch at `F` from the floor corner to
   the junction, inside the reach.
8. **Offset strategy** (`vcarve.ts`): `step_over_mm` in (0, ∞), else
   `CamError::InvalidVCarveStepOver` (#407: `InvalidStepOver` is a
   fraction in (0, 1]). Ring `k` is each region inset by
   `k · step_over_mm` (`floor_regions`, round joins, from the region
   itself at every level so no error adds up; a level that is noise at
   the flatten tolerance, Pocket's rule, is no ring), at `d_k = k ·
   step_over_mm / slope`, `z = start_plane − d_k`, until the inset is
   empty or would reach `F · slope` (within `EPS_MM`). The ring at
   `F · slope`, `z = start_plane − F` exactly, is the floor's boundary
   trace, cut by `floor::blocks` after the V-bit floor inside it (step 6,
   #405), from the last ring's end. The deepest ring, or the floor when
   there is one, is step 3's deepest point. Each
   ring is oriented by `ring_is_ccw(wall, direction)` as Pocket's rings
   (re-derived for Y-up there; an outer's ring is a `Pocket` wall ring, a
   hole's an `Island` ring) and started at its point nearest the
   previous ring's end, level by level as upstream; the region's first
   ring at its first point. Arcs become lines within the flatten
   tolerance on the side away from their wall (`floor::flatten_inside`:
   chords when the arc turns toward the inside, tangents when its centre
   is on the wall's side), so no tip comes nearer a wall than its ring. A run of
   rings joined tool-down is one `CutBlock` (as a run of medial chains),
   `cleared` `None`, `entry_z_mm` its first ring's Z. A link between
   rings of one region stays down only when it passes the cone test at
   its own depth (`Walls::least_room` along the straight move from the
   previous ring's end to this ring's start, each at its Z), else
   retract; upstream bridges within 8 × stepover on the surface disc,
   which DreamCarve does not trust. Every point of a ring is on the final
   V surface (its cone touches the wall), so a medial link's `EPS_MM` to
   spare would retract every ring: a ring link is held to the surface
   the rings cut, no point below it by more than `EPS_MM` (#407). On a
   straight wall the link from a ring's corner to the next ring's corner
   runs down the corner's bisector, which is that surface, and stays
   down; elsewhere it retracts. No corner tips: the rings are round.
   More than `MAX_RINGS` (Pocket's) rings in a region is
   `InvalidVCarveStepOver`.
9. **Sequence.** `SequenceInput` with `stepover_mm` 0 and every medial
   and offset block's `cleared` `None`, so `sequence` itself never links
   two of them tool-down (the floor's rings carry `cleared`, step 6); containment pairs from `nest`; the operation's
   `machining_order` (one pass, so both orders cut the same; `pass_first`
   is accepted and equal). The descent to each entry is a straight plunge
   at the plunge rate from the entry clearance above the **stock top**
   (not the start plane: S2 unchanged) to `entry_z_mm`, as `sequence`
   does for every plunge block.
10. **S17 on the toolpath.** Before returning, the generator runs
    `vcarve::cone_in_material` on its own toolpath (the exact test of
    "Safety pass", with the vertical-descent exemption) and turns a
    violation into `CamError::ConeOffMaterial`: a library crate never
    hands back motion it can prove wrong. `assert_safe` runs the same
    function in every generator test. Each limit allows `tol` (flatten
    tolerance plus `EPS_MM`), so a cone that meets a wall exactly, or a
    tip on an outline at a move's end, passes
    (`vcarve::touching_a_wall_is_not_a_violation`). The overlap a
    violation reports is read where it is worst: for a tip that leaves
    the region, at the point of the move farthest outside the walls, not
    at an end or a crossing, where it reads 0 (#463: "0.0000 mm"). Every
    reported overlap is therefore more than `tol`, at least 0.01 mm,
    and shows at the message's four decimals
    (`vcarve::real_wall_violation_is_still_refused`). An arc move, which
    no V toolpath has and the test is not written for, is
    `CamError::ConeArcMove`, with no distance.

### Clearing passes (dc-cam::vcarve::clearing)

A carve with `clearing` passes produces one `ToolRun` per pass, then its
own. `vcarve::clearing::checked_tools(project, op)` checks the passes, and
`vcarve::clearing::runs(project, op, carve, tools, slope, &mut warnings)
-> Result<Vec<ToolRun>, CamError>` cuts them (#406: the tools checked once,
`slope = tan(α/2)` rather than the half angle):

1. Each pass's tool must be a flat end mill with a usable diameter
   (`ClearingInvalid`, the validation reasons re-checked;
   `InvalidToolDiameter`); the carve must have a flat depth. Its rates are
   the Pocket code's to check. A tool wider than the previous pass's warns
   `ClearingNotDescending` and is not refused; it reaches nothing a
   narrower end mill before it left (a disc reaches nothing a narrower
   disc cannot), so it also warns `ClearingNoRoom` and has no run (#406).
2. Pass `i`'s regions are the carve's **floor regions**
   (`dc_geometry::floor_regions` with the carve's V-bit angle, less
   noise, as `Floor::regions`), and it rests on passes `0..i` as a
   `rest_of` Pocket rests on its reference (#406, corrected from "minus
   the reach of passes `0..i`"): from their tools' radii it works out what
   they left (`rest::left_by`) and keeps only its tool centres within its
   radius of that (`rest::rest_centre`), so its footprint may run into
   what they cleared but every centre touches material they left. A
   Pocket of `floor − reach` itself would stand the smaller end mill a
   radius off the reach's edge and leave the corners it is there for. A
   pass that reaches nothing of any floor region (no room for its end
   mill, or nothing left that it touches) warns `ClearingNoRoom` and
   emits no run. Its depth is `start_depth + F`, its `pass_z`
   levels from its tool's step-down (the end mill steps down; the V-bit
   does not), its `stock_to_leave_mm` 0 (the floor boundary already stands the
   V-bit's inset off the outline: Corbin's `pocket_stock_to_leave` is
   that inset, which `floor_regions` has applied).
3. The pass is cut by the Pocket code: the generator builds a `PocketOp`
   in memory (the pass's tool, strategy, direction and entry; wall pass
   `last`; no corner relief; the carve's `machining_order`,
   `safe_z_mm` and through-cut allowance) and hands it, with the regions
   of step 2 as resolved targets, to the same ring, raster, entry and
   sequence code a user's Pocket goes through. The only change in
   `pocket.rs` is an entry point that takes resolved regions instead of
   instance ids: `pocket::pocket_regions(project, op, regions, shapes,
   reached_by, &mut warnings)`, where `shapes` names the regions'
   contours by index (the floor's outers by their carved region's shape;
   its grown islands by none, so no `pocket_island_too_close` is warned
   for them) and `reached_by` are the earlier passes as rest references.
   `generate_pocket` is that entry point after resolving and nesting its
   targets, so phase 2's programs are unchanged. One carved region's
   floor can be several pieces, one in another's island (#482: the inset
   closes a bay's mouth). They share the region's shape, so no
   containment pair orders them (`sequence` would read a shape paired
   with itself as a containment cycle); the regions come in with each
   shape's pieces together, shallowest first, and `sequence` cuts a
   shape's blocks at a pass in that order, so a piece is still cut after
   the piece round its island.
4. The runs come back in list order followed by the V-bit run, each
   with its own tool. `export`'s `build` turns each run into an `EmitOp`
   (tool = the run's tool, `holes` `None`, `label` the run's);
   `emit_programs` already groups consecutive `EmitOp`s by tool, so a
   carve with two clearing tools and a V-bit is three tool runs, and the
   sign's `op2` with `clearing: [t1]` after a T2 Drill gives the
   programs T2, T1, T3, T1. `order::check`, the `tool_change_avoidable`
   hint, `check`'s summary and the preview legend take runs and name
   them by label (`RunLabel::naming`: `op2 (clearing t1)`; a program
   comment cannot hold parentheses, so it reads `Operation 1: Sign
   clearing t1`). `order::check` is given each operation's own run (step
   5), and the hint counts every run from the project
   (`Operation::run_tools`: the clearing passes' tools, then the
   operation's), a pass with no run included. The safety pass holds a
   clearing run to the floor regions it re-derives (S9 bullet); until
   #408 it fails closed on a carve with clearing passes, so `check` and
   the preview show the runs and `export` refuses them.
5. `order::check` sees the clearing runs as part of their operation: they
   cut inside the carve's targets before the carve, which is the
   intended order, and warn nothing. The carve's `cleared_by` and the
   pass's regions come from the same `floor_regions` call on the same
   inputs, so the two sides never disagree.

### Start depth, `start_depth_mismatch` and `start_depth_uncleared` (dc-cam::order)

For every enabled V-Carve, Photo V-Carve and countersink Drill with its
start depth `s` (an Inlay is exempt: the female's start plane is the
stock top and the male's is the plug's own design, with the material
above it kept on purpose; its flank engagement is counted from the stock
top instead, "Inlay" step 1):

1. Its **footprint outlines**: a carve's target outlines, a Photo's
   footprint rectangle; for a countersink, the disc of its **tool's
   radius** at the centre of each target circle (#425), not the hole
   circle: the bit's widest section above the start plane, in the
   Pocket's air, so a Pocket that holds the hole but not the bit round it
   would let the bit cut its wall. Every circle the Drill finds counts,
   one it skips for `countersink_mouth_not_wider_than_hole` included;
   a target that is no circle is not cut and is left out. A disc is held
   when its centre is in a region and at least the radius, less the
   flatten tolerance, from the region's outline and islands, arcs as arcs.
   The footprint is one enum in `dc_project::cleared_start_plane`, so a
   new kind of cleared floor adds an arm for the clearing operation
   without touching the footprints.
2. The **cleared start plane** (`dc_project::cleared_start_plane`,
   which validation, `order::check`, the generators and the safety pass
   all call; it is in `dc-project` because validation may not depend on
   `dc-cam`, #413; it was `containing_pocket` until #426). It returns a
   `ClearedPlane { by: &Operation }` with `depth_mm()` and
   `is_at(s)` (within `EPS_MM`): the last operation earlier in the list
   that is either
   - an **enabled Pocket** whose regions contain every footprint outline;
     its plane is its `depth_mm`; or
   - an **enabled V-Carve with a flat depth `F`** (#426, decided
     2026-10-29) whose **floor regions** contain it: its targets' regions
     inset by `F · tan(α/2)` with its own V-bit (`dc_geometry::
     floor_regions`, less the pieces whose area is at most the flatten
     tolerance times their perimeter, as the generator's floor and the
     safety pass's clearing-run check derive them). Its plane is
     `s0 + F`, its start depth plus the flat depth (`Operation::
     depth_mm`). The V-bit's floor rings and any clearing passes cut the
     whole floor (#405, #406), and above it the carve's walls slope
     outward, so a footprint inside the floor at floor level is in air
     all the way up. A disabled V-Carve, a later one, one without a
     flat depth, or one whose floor does not resolve (no V-bit with an
     angle in (0°, 180°), a start depth below 0) does not count. Known
     limit: where the V-bit alone cuts the floor, its rings leave ridges
     up to `step_over_fraction · F` high (#405), which this rule does not
     count as material; an operation started on the floor engages them
     with its flank up to that much above its plane. The V-Carve chapter
     recommends `step_over_fraction` 0.1 (0.25 mm ridges at `F` 2.5 mm)
     or a clearing pass.

   Contain means each outline's vertices inside a region, outside its
   islands, by `Region::contains_point`; an outline crossing a region
   boundary does not count, which the area the outlines enclose less the
   clearing regions, within the flattening slack, decides. The rule
   applies to every start-plane operation: a countersink on a V-Carve's
   floor (the sign's hanging holes) and a V-Carve carved into another's
   floor alike.
3. With a cleared plane of depth `d ≠ s` (within `EPS_MM`): warn
   `StartDepthMismatch (op, cleared_by, floor, s, d)`, naming the Pocket
   or the V-Carve. The user's mistake is visible and the cut is in
   material the project reasons about, so a warning.
4. With none and `s > 0`: the error `start_depth_uncleared`, unless the
   operation says `start_plane_cleared_externally: true`, in which case
   it is quiet and the flank engagement counts from the stock top
   (security review M2, decided 2026-10-28; the draft warned and
   exported). The one case the project exports a cut it cannot see the
   material of is now the user's explicit statement, and even then the
   shank check holds.
5. With none and `s = 0`, or a match: quiet.

`order::start_plane(project, op)` classifies an operation as stock top,
cleared (a match), mismatch, cleared externally or uncleared, from one
list of start-plane operations (`start_setting`, an exhaustive match on
`Operation`: V-Carve in 3.1; the countersink Drill and the Photo V-Carve
join it in 3.2 and 3.4). `order::check` warns only the mismatch, read
from the project's enabled operations, not from the generated toolpaths;
`check` and `export` print it at the operation's `start_depth_mm`. The
uncleared case is reported once, by validation: `export::build` stops at
a validation error, so neither the generator's `StartDepthUncleared` nor
`order::check` runs on such a project (#409).

### Countersink (dc-cam::drill)

In `generate_drill`, `DrillCycle::Countersink { diameter_mm: D,
start_depth_mm: s }`:

1. Tool a V-bit with α in (0, 180), else `CountersinkNeedsVBit` /
   `VBitAngleRange`. `D > 0` else `CountersinkDiameterPositive`, `D <=
   diameter + 1e-9` else `CountersinkMouthExceedsTool`. `depth = D / (2 · tan(α/2))`
   (`countersinkTipDepth`); `depth <= cone + EPS` holds by construction
   when `D <= diameter` (upstream's `maxCutDepth` tolerance note applies:
   compare with `1e-9`). With `s > 0` the flank engagement is `s + depth`
   unless an earlier Pocket or V-Carve floor at `s` has cleared the start
   plane (`dc_project::cleared_start_plane`, as V-Carve step 3), and `s + depth <=
   cone + 1e-9` else `CountersinkConeExceeded` (#425); no Pocket and
   `start_plane_cleared_externally: false` is `StartDepthUncleared`
   (security review H4, M2).
2. Targets as Drill (circle test); a circle whose diameter is at least
   `D − 1e-9` is skipped with `CountersinkMouthNotWiderThanHole`. Drill's
   phase 2 rule that skips a hole narrower than the tool (format 17,
   `allow_tool_wider_than_hole`) does not apply: a countersink's V-bit is
   wider than its hole by design, and the mouth-versus-hole rule replaces
   it (changed in spec sync 1). The field is ignored for a countersink
   cycle and validation says so if it is `true` (`countersink_ignores_tool_width`, a warning).
3. `top = thickness − s`; `bottom = top − depth`; S1 with the allowance.
   `retract_z = min(safe_z, thickness + max(retract_mm, 0))`: measured
   from the **stock top**, not the start plane, so the hole's single
   `Rapid` never ends below the stock top (S15) and the plunge covers the
   pocket's air at the plunge rate (security review H3; the draft
   measured it from the start plane, which put the rapid `s −
   retract_mm` into the stock). One block per hole as Simple: `Rapid` to
   `retract_z`, `Linear` to `bottom` at plunge, `Dwell` if `dwell_s > 0`,
   `Rapid` to safe Z. `entry_z_mm = retract_z`.
4. `DrillHole::cycle = Expanded` always, so no `G8x` is written whatever
   `use_canned_cycles` says (S18).
5. Order, S15 and the rest as Drill. `drill_cutter_unusual` is not raised.

### Inlay (dc-cam::inlay)

Both boards are a `vcarve::Carve` plus checks; the generator is
`vcarve::blocks` and `sequence`.

**Boards** (#498; maintainer decision 2026-11-02, from the phase 4 UI
review, #460). One Inlay cuts both boards of the pair from one set of
values, so they cannot drift apart: until format 25 an `InlayOp` cut one
`side`, and a pair was two project files kept in step by hand.

- The **female** is cut on the project's stock, which stays the female
  board, so it can carry other operations (a Pocket, an outline
  Profile). Its runs are `Board::Female`.
- The **male** is cut on the Inlay's `male_board`, generated on
  `project.on_male_board(op)`: the project with its stock replaced by
  `MaleBoard::stock`, the board's width and height (the stock's where
  `null`) and its own thickness. The male board shares the stock's
  bottom-left corner, so the design is mirrored in place on it about the
  same line as before, and the origin preset applies to it as to the
  stock (bottom-left of that board, centre of that board, Z at that
  board's top or bottom). A male board the size of the stock gives, byte
  for byte, the programs a male project gave on that stock. Its runs
  are `Board::Male`: clearing passes, the V-bit, the cut-out, in that
  order.
- Every rule that reads the stock for the male reads the male board: in
  steps 1 and 4 to 6 "the stock", "the stock top" and `thickness` are the
  male board's. So the cone's engagement `D − G + P` is from the male
  board's top; the band is clipped to `MaleBand::board` of the male
  board, and `InlayTooNearEdge` names the male board's edges and length;
  the cut-out goes through the male board, its tabs counted from its
  bottom; and `InlayPlugBaseThin` reads the male board's thickness. The
  safety pass holds male programs to the male board in the same way (S1,
  S7, S17's stock-top plane, the cut-out's floor ring; "Safety pass").
- While `male_board.thickness_mm` is `null` the male is not cut: it has
  no runs, its own rules (its margin, cone, cut-out and plug base) wait,
  and the generator warns `inlay_male_board_unset`, located at
  `operations[i].male_board.thickness_mm`. The female still exports.
- `generate_inlay` returns the female's runs, then the male's; the
  male's `inlay_parts_merged` and `inlay_hole_filled` are left out (the
  female's already name the same parts, where they are drawn), and so is
  any warning the female already gave.
- Every male of a project is cut on one male board
  (`Project::male_board`, the first enabled Inlay that cuts its male):
  validation refuses a later Inlay whose board differs
  (`inlay_male_boards_differ`), and the emitter does too
  (`MaleBoardsDiffer`). Two Inlays' males are cut on that board in
  operation order.
- The order check (`cut_inside_freed_part` and the others) reads the
  stock only: an Inlay's female as drawn. `tool_change_avoidable` is
  worked out per board: on the stock every operation's runs but Inlay
  cut-outs, on the male board each Inlay's clearing passes, V-bit and
  cut-out.

1. **Values.** `depth_mm` D, `glue_gap_mm` G, `plug_depth_mm` P required
   (`MissingDepth`-style errors naming the field); `0 <= G < D` else
   `InlayGlueGap`; male: `margin_mm` M `> 0` else `InlayMargin`;
   `outline_rounding_mm` R, when set, `> 0` else `InlayValueRange`
   naming it (#448). Tool
   checks as V-Carve. The cone check is on the flank's **engagement from
   the board's top**: `D` for the female, and `(D − G) + P` for the male,
   whose board above the start plane is the plug and is never cleared
   (security review H4: the draft checked `P` alone; with the bench's
   90°, 6.35 mm bit, cone 3.175 mm, Corbin's `D 2.5, G 0.25, P 1.5`
   engages 3.75 mm and would have run the shank into the plug wall).
   Validation checks the same sums. The inlay test part's `4, 1, 2`
   engage 3 + 2 = 5 mm against the 60° bit's 5.5 mm cone.
2. **Design regions.** Targets nested as V-Carve's (step 2), then
   **near parts merged** (#479; `dc_geometry::merge_near_parts`, which
   the generator and the safety pass both call). Two parts whose outlines
   come closer than the merge distance `d` across a gap outside the
   design (two outers side by side, or a hole and an island directly in
   it; not two holes, nor a hole and its own outer, which have the
   design's material between them) are one shape on both boards. A V-bit
   cannot carve such a gap: the female left a wall standing to the stock
   top between the two pockets, the male cut no groove between the two
   parts of its plug, and the plug stood up to `D − G` proud on the
   wall (found by the #464 stress tests: 3 mm on two squares 0.001 mm
   apart, 2.06 mm at a circle 0.005 mm from a square). `d` is
   `dc_project::INLAY_MERGE_MM`, 0.05 mm, the most the maintainer allows
   (decided on #479). A gap under `2·tol + √2·q` (0.0214 mm by default;
   `tol` the project's flatten tolerance, `q` a unit in the last place
   written) cannot be carved at all: each outline is carved as chords up
   to `tol` off it, so two outlines closer than `2·tol` can meet in what
   the carve follows (the circle beside the square failed to seat with
   gaps up to 0.02 mm and seated from 0.0205 mm), and a tip path keeps
   `q/√2` from each wall (#469), so a gap needs `√2·q` more to hold one
   (the squares failed 0.0012 mm apart and seated 0.0015 mm apart). That
   bound was tried as `d` and was not enough: near touches up to 0.044
   mm wide still held the plug up to 1.6 mm proud, where the carve does
   not reach into the narrow wedge either side of the gap (the stress
   sweep's line art seed `0x32`, 0.0436 mm, and a square's corner 0.022
   to 0.035 mm from a circle, 1.05 to 1.11 mm), and merged, the wedge a
   closing by 0.0107 mm leaves past the fill was still too narrow at
   some gaps (the corner, 0.016 mm: 0.70 mm). With `d` 0.05 mm every one
   of them seats at every gap tried from 0.001 to 0.048 mm (within 0.021
   mm). The smallest `d` that seats them all lies above 0.0436 mm, and
   the stress tests draw near touches up to 0.05 mm. Outlines that touch (within
   `EPS_MM`) are separate parts, as `nest` takes them. A group of near
   parts is closed by `r = d/2 + f` with round joins, `(X ⊕ r) ⊖ r`; the
   pieces the closing adds against two outlines (the gaps; not those
   against one, an inward corner rounded) are grown by `f` into the parts
   and held to the closing, so they overlap the parts rather than meet
   them along edges a rounding error apart, and united with them; and the
   slivers the booleans leave, a region or hole of area at most `f` times
   its length, are dropped (left in, a sliver hole was a wall inside the
   merged letter, and the generator refused its own cut past it; and the
   closing less its corner pieces, used at first, left outlines touching
   themselves). So the parts keep their shape but at the gap, where the closing's arc dips `r − √(r² −
   (g/2)²)` into a gap `g` that opens between two outward corners. Where
   two outlines part (a circle beside a square), the closing ends the
   fill with an arc of radius `r`; its vertices off the parts (more than
   `1e-5` mm from them as the booleans flatten them) are cut to the one
   farthest from them, so the fill ends in a corner, which the medial
   axis reaches (`CornerReach::Every`). Ended round, the fill left the
   male's V-bit half a millimetre short in the narrow wedge beyond it at
   some gaps (the circle 0.009 to 0.013 and 0.019 to 0.021 mm from the
   square, with `d` at 0.0214 mm), and the plug stood 0.9 to 2 mm proud.
   The
   group is flattened within `f`, the finer of the V-bit's wall tolerance
   (`dc_geometry::wall_tolerance_mm`, #478) and `d/8`, so the
   flattening cannot reopen a gap `d` closes; regions with no near part
   keep their arcs, and a design with none is unchanged. Each pair of
   outlines warns `inlay_parts_merged` with the gap and its middle as
   drawn. The male merges its mirrored design, the same rule on the same
   shapes. A gap of `d` or more is left as drawn: two squares 0.1 mm
   apart keep the ridge between the female's pockets, and the pair seats.
   One outline that comes within `d` of itself across the outside is
   merged the same way (#515): two stretches of it at least `8·d` apart
   along it whose gap's middle lies outside the region, where the
   region's own closing adds a piece against both stretches, two separate
   runs of the outline; a piece that fills a notch's tip or rounds an
   inward corner touches one run and is not kept, and a fold across the
   design itself (a thin web of it) is never one, as the closing only
   adds outside. Lettering `0xaf`'s "m" and "e" overlap, so they are one
   outline, which comes 0.02 mm from itself between the "m"'s leg and the
   "e"'s curve; its male stood 1.263 mm proud
   (`stress::lettering_outline_near_itself_is_merged`,
   `inlay::self_near_touch_is_merged_and_seats`). It warns
   `inlay_parts_merged` with the shape as both parts, worded for one.
   A hole narrower than `d` all along (its own area inset by `d/2` is
   empty) that holds no other region is filled, the same thing the other
   way round (#511): a hairline where two overlapping letters leave a
   sliver. The female left a wall that thin to the stock top, the male's
   groove keeps half a written unit from each wall and was dropped, and
   a 3 µm slit's plug stood 3 mm proud in an inch program
   (`inlay::hairline_hole_is_filled_and_seats`). Each hole warns
   `inlay_hole_filled` with its mean width (twice its area over its
   perimeter) and the middle of its box as drawn.
3. **Female.** `Carve { regions: design, start_plane: stock thickness,
   flat_depth: Some(D), strategy: Medial, cleared_by: the
   operation's clearing passes }`. Identical to a V-Carve with `flat_depth_mm: D`,
   `start_depth_mm: 0`, which acceptance 56 checks move for move.
4. **Male.**
   - `male_mirror`: the joint bounding box of the targets' outlines in
     project space, centre line `x = cx`; `Transform::mirror_x_about(cx)`
     (`[−1, 0, 0, 1, 2cx, 0]`). Applied to every target outline;
     `Contour::new` renormalises winding (a mirror reverses it; a mirror
     negates bulges, which `Transform` already handles). `check` and
     `export` print `inlay_male_mirrored` with `cx`.
   - **Edge rule** (#448; `dc_project::male_band`, which validation,
     the generator and the safety pass all call, as
     `cleared_start_plane`). With `s = tan(α/2)`, `r` an end mill's
     radius and `R` `outline_rounding_mm` (0 when `null`): the **edge
     pass** is the first clearing pass whose end mill fits the floor
     ring, `M − P·s ≥ 2r` (`dc_project::edge_pass`). The band's outer
     edge is **open** when there is one and the V-bit's flank at the
     plug's foot stays inside the band at the stock top, `M ≥ (D − G +
     2P)·s`; the band is then closed by `R' = max(R, r_edge)`, so the end
     mill reaches every inward corner. Otherwise the edge is a V wall,
     `R' = R`, and with clearing passes the generator warns
     `inlay_band_edge_v_wall` naming `max(P·s + 2·r_min, (D − G +
     2P)·s)`. (The flank condition is not in #448's text: the issue asked
     for an open edge whenever a pass fits, but a V-bit tracing the
     plug's foot `P·s` out cuts `(D − G + P)·s` wide at the stock top,
     and where the band is narrower than that its flank would chamfer
     the full-height board beyond the edge, which the safety rule
     forbids. The tutorial's 4.04 mm and the bench's values are far
     inside any usable margin.)
   - **Band** (`dc_geometry::rounded_band`): `B = ((design outers) ⊕ (M
     + R')) ⊖ R'`, a morphological closing with round joins: inward
     curves tighter than `R'` become arcs of radius `R'`, notches and
     gaps narrower than about `2R'` fill, and `B` reaches no farther than
     the plain band where it is convex. `R' = 0` is the union of the
     offsets, as before. `B` has no holes (#484): where the parts' margins
     ring round board farther than `M + R'` from every part, that board
     is band too. No part lies in such a hole (each part's margin covers
     it), and left out of `B` it was never cut: it stood to the stock top
     inside the band's floor and held the plug off the female's surface,
     up to 2.6 mm on the sweep's lettering
     (`carve::band_fills_the_board_its_parts_ring_round`,
     `inlay::band_round_a_space_seats_flush`). The safety pass reads the
     same `B`. `dc_geometry::surround` is `B` minus
     `design_mirrored` (every region, so a hole in the design is carved:
     `difference` with the design's regions, which subtracts their outers
     and adds back their holes, with walls on every side). Tolerance the
     project's for the band's edge; the design's outlines, the plug's,
     within the bit's wall tolerance (`dc_geometry::surround_within`,
     `wall_tolerance_mm`; V-Carve generator, step 6; #478): drawn as
     chords within 0.01 mm the star's circle held the plug 0.072 mm proud
     with the 15° bit. A band clipped to the board (#495) loses the
     design the same way (`dc_geometry::difference_within`), and the
     safety pass re-derives the surround the same way.
   - `Carve { regions: surround, start_plane: thickness − (D − G),
     engaged_from: thickness, flat_depth: Some(P), open_edge, … }`.
     **Open edge** (`vcarve::OpenEdge { band: B, design, top_z_mm }`):
     the edge pass clears the floor out to `B`, so its outermost centre
     path is `B ⊖ r` and the edge is a vertical end-mill wall from the
     stock top to the floor. The floor of a band region keeps `B` and
     loses only the design's outers grown by `P·s`
     (`dc_geometry::surround_floor_regions`); a design hole's floor is
     inset all round. The V-bit carves only the walls against the design
     and the floor no pass reaches: its medial chains are cut to the
     stretches whose cone stays inside `B` at the stock top
     (`medial::runs_within`), its floor rings are clipped to `B ⊖ (D − G
     + P)·s`, and its boundary trace runs along the plug's foot only,
     never round `B` (`floor::blocks_open`). A floor piece no pass
     reaches is cut whole within `B ⊖ (D − G + P)·s`, noise dropped only
     below the wall tolerance, the floors' own rule (#506, #565): its
     medial chains are dropped, and at the project's tolerance a piece
     under 0.02 mm wide in a channel between two strokes was cut by
     nothing, and line art `0x63`'s plug stood 0.101 mm proud on it
     (`inlay::open_edge_male_cuts_a_floor_piece_a_hair_wide`). `finish` runs
     `cone_in_material` a second time against `B` with the stock top as
     the plane. **V wall** (`open_edge: None`): the surround's outer edge
     gets a V wall too, as before. Changed in #448 after the
     maintainer's preview of a lettered male (2026-10-31): the V wall at
     the band's edge (once "harmless", to match a VCarve boundary vector)
     wasted V-bit time on waste wood and kept the clearing end mill `P·s`
     plus the floor's ring short of the edge, and the plain offset copied
     every notch of the design (the dip over an M, the pinches between
     letters). A user-drawn boundary is #449.
   - **Clipped to the board** (#495; maintainer decision 2026-11-02: the
     band only gives the plug room, and nothing outside it matters).
     Where `B` runs past the stock rectangle it is clipped to the board,
     `B ∩ board` (`dc_geometry::band_on_board`, an intersection, flattened
     within the tolerance as `B` is), and the surround is the clipped band
     less the design. The board is `MaleBand::board`: the male board's
     `[0, W] × [0, H]` (the stock's before #498) with an open edge, so the edge pass clears the floor to the
     board's edge, its end mill's edge on it and never past it (S7 allows
     a cut exactly along an edge); and that rectangle inset by the V-bit's
     radius `R_v` with a V wall, since the V-bit runs down the wall to its
     top and S7 holds it to its full radius. Where `B` stays on the board
     (its outlines within `EPS_MM` of it) nothing changes: the surround and
     the open edge are built exactly as before, so every unclipped program
     is byte for byte the same (`inlay::unclipped_band_is_unchanged`). The
     generator warns `inlay_band_clipped`, naming the edges `B` runs past
     and the narrowest band left: the least distance from the design to a
     clipping edge, less the inset. With an open edge the plug's rim there
     is the board's edge. The open edge's `B` (the V-bit's cone at the
     stock top, the floor's outer edge) is the clipped band. The design's
     own cut is never clipped: where a stock edge clips the band, the
     design must stand far enough from it for what the band holds there
     (`MaleBand::board_room_mm`), else `InlayTooNearEdge` names the edge, the
     distance, the room needed, the move away from the edge that fits and the stock length
     along that axis that fits the design centred (both rounded up to
     the thousandth shown). The room is the most of: the V-bit along the
     plug's foot with its whole radius on the board, `P·s + R_v` (which
     holds its flank as well, since the cone check keeps `(D − G + P)·s ≤
     R_v`); with an open edge, the edge pass's floor ring `P·s + 2r` and
     the flank `(D − G + 2P)·s`; and with a cut-out, `R_v` (a V wall only)
     plus its floor ring, `MaleBand::cut_out_margin_needed_mm`. The rule
     is the band's own with `M` replaced by the band left. The issue's
     example, a design 2 mm from the edge, needs a V-bit under 4 mm
     across with the bench's angles, since S7 reads a V-bit's whole
     radius; the bench's 6.35 mm 60° bit needs 4.33 mm at `P` 2. Found on
     the maintainer's eagle male: its stock is the drawing's size, 10 mm
     round the design, with `M` 16 and `R` 25, so `B` ran 6 mm past every
     edge and export failed with "cuts off the stock"; clipped, it
     exports.
   - **Every corner of the plug (#452).** The male's flank is engaged
     from the stock top, so wood stands above its start plane wherever
     its carve does not reach: a corner of the surround the medial axis
     misses keeps wood up to the stock top, where a chain ending at the
     corner, its tip at the start plane, would cut a disc of radius `h ·
     s` at height `h` about it, exactly the plug's inset there. The
     carve reaches every convex corner of the plug's outline and fits
     its chains to their own depth (V-Carve generator, step 5, "Fit";
     medial axis, "Corner reach"). Found by the maintainer's bench check
     (#438): a serif "M" whose stem and diagonal meet in a 23° notch
     lost the notch's whole branch to spur pruning, and the plug stood
     3 mm proud. `inlay::plug_seats_flush` simulates both boards' programs
     as heightfields and seats the plug: the male's share of the seat
     must be within 0.05 mm, and since #457 so must the whole seat (the
     female's Douglas-Peucker chords, a quarter step off the axis,
     stopped up to 0.11 mm short of a wall on the "DC" letters, which
     held the plug 0.185 mm up; it now seats 0.011 mm up).
   - `InlayPlugBaseThin` when the male board's `thickness − (D − G + P)
     < 6`.
5. **Fit property** (acceptance 57; `inlay::female_and_male_walls_mate`):
   for sample points `q` on a design outline and heights `h` in `[0, D −
   G]`: the female's uncut cross-section at depth `h` is the design inset
   by `h · slope`, and the male's at height `h` above its start plane (`z
   = start_plane + h`) is the mirrored design inset by the same, read
   from the two toolpaths' cone discs. The test builds both toolpaths,
   mirrors the male's back, and checks that the carved discs of each
   toolpath touch (within 0.02 mm) the same outline at the same inset.
6. **Cut-out** (the male's, on its board, format 23, #443; Corbin's optional "Plug
   Cutout"; moved by #448 and #461). With `s = tan(α/2)`, `P`, `M`, `X`
   the mirrored design, `B` the band's edge (step 4, closed by `R'`) and
   `r` the cut-out end mill's radius, the band's flat floor ring runs
   from the plug's foot, `X ⊕ P·s`, out to `B` with an open edge, or to
   the V wall's foot, `B ⊖ P·s`, with a V wall. The tool centre runs as
   far out as the floor reaches, so that above the floor it cuts nothing
   the carve did not remove: on `B ⊖ r` with an open edge, its outer
   edge along the edge pass's upright wall; on `B ⊖ (P·s + r)` with a V
   wall, its outer edge along the slope's foot, so the freed plug has no
   knife edge (`MaleBand::cut_out_inset_mm`, `r` or `P·s + r`;
   `dc_geometry::inset_outers` of `rounded_band(X, M, R')`: inset as
   `floor_regions` insets, outers shrunk and holes grown, and only the
   outers kept, so a hole of `B`, board no carve reached, is not cut
   round; `inlay::cut_out_paths`). Before #461 it ran in the middle of
   the floor, `c = (M + P·s) / 2` (open) or `M / 2` (V wall) out from
   `X` closed by `c + R`, which left a strip of floor outside it; the
   maintainer's preview (2026-11-01) asked for it as far out as it can go
   without cutting more. **Room** is unchanged: `InlayCutOutNoRoom`
   unless `M − P·s ≥ 2r` with an open edge (naming `P·s + 2r`) or `M −
   2·P·s ≥ 2r` with a V wall (naming `2·P·s + 2r`;
   `MaleBand::cut_out_margin_needed_mm`). Both keep the cutter at least
   `r` outside the plug's foot: `B ⊇ X ⊕ M` (a closing only adds), so
   every point of `B ⊖ i` is at least `M − i` from `X`, and `M − i − r ≥
   P·s` is the condition. Design holes are not cut out. The paths are
   cut by the Profile code as an On
   line Profile (`inlay::cut_out_profile`: the paths as one definition
   placed in place, climb, plunge entry, the cut-out's tool, tabs and
   allowance, depth `thickness + allowance`), with its passes starting at
   the floor, `D − G + P` below the stock top
   (`profile::generate_profile_below`): no pass cuts the air the carve
   removed. Tab heights count from the male board's bottom. The run is
   `RunLabel::CutOut(tool)`, the operation's last, after the V-bit's, so
   the piece is freed only once carved (`op1 (cut-out t2)`; with the
   clearing end mill the tool is fitted twice, and
   `tool_change_avoidable` is expected). The star (`M` 8, `P·s` 1.155)
   with the 3.175 mm end mill's clearing pass: the centre runs 1.588 mm
   inside `B`, 6.41 mm out from the star where `B` is plain (4.577 mm
   before #461); without it, 2.742 mm inside `B`, 5.26 mm out (4 mm
   before).

   **On a clipped band** (#495). The path is the clipped band's outers
   inset as above, `(B ∩ board) ⊖ i`, which is `(B ⊖ i) ∩ (board ⊖ i)`:
   the unclipped path where it stays on the board, and stretches along
   the board inset by `i` where it does not. With an open edge (`i = r`,
   the board the stock itself) those stretches run the radius inside the
   board's edge, where the board's own edge frees the plug, so they are
   dropped (`dc_geometry::without_rect_sides`): the rest are open paths
   from the board's edge to the board's edge, through the stock, their
   ends the radius inside it, so the kerf reaches the edge and the tool
   never leaves the board. They are cut by the same On line Profile,
   which cuts open paths as drawn (#175), its plunge entry at a path's
   start; its tabs go only on those paths, laid from each path's start
   as an open path's are (an open path may have none). With a V wall the
   band is clipped `R_v` inside the board and the board stands beyond
   it, so the path stays closed and frees the plug from it. Room as
   step 4.

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
4. **Blocks.** One `CutBlock` per groove: `entry` the first sample's XY
   and `entry_z_mm` its Z, `descent` empty (straight plunge at the plunge
   rate to `entry_z_mm`), `cuts` the samples as `Linear` moves at their
   own Z, `exit` the last sample. Between grooves: a `Linear` rise to the
   **link height** at the feed rate, a `Linear` move at that height to
   the next groove's start, then the plunge. The link height is
   `start_plane + stepover_retract_mm` when a containing Pocket has
   cleared the start plane, and `max(start_plane + stepover_retract_mm,
   thickness + ENTRY_CLEARANCE_MM)` when the user vouched for it
   (`start_plane_cleared_externally`), so the link never runs through
   material the project has not seen (security review L1; RasterCarve's
   lifts are not compared, so parity holds). `sequence` is told
   (`SequenceInput::stay_down_links: Some(height)`) to link consecutive
   blocks of one group at that height with feed moves rather than retract
   to safe Z (a new field, read only by Photo; every other generator
   leaves it `None` and is unchanged). The first groove and the last
   retract use safe Z as every operation does. Grooves of one family are
   one `group` so `sequence` keeps their order. `MAX_PHOTO_MOVES` bounds
   the sample count before anything is allocated (a `sample_mm` or
   spacing of `1e-9` in a shared file is refused, not run).
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

**Per board** (#498). Each program is checked against the board it cuts
(`Program::board`, `Material::of_run(…, board)`). A male board's programs
are checked against `project.on_male_board(inlay)`, so every check that
reads the stock reads the male board: S1 holds them to the male board's
bottom (the cut-out's own through-cut allowance below it), S7 to its
rectangle, S17's stock-top plane is its top, the cut-out's floor ring is
derived on it, S9 places the cut-out's tabs from its bottom, and the
origin preset and safe heights are its own. The stock's programs are
checked as before; an Inlay's female is held to its design as drawn. A
male program is never checked against the stock, nor a female one
against the male board.

- **S17, cone.** For a V-Carve, an Inlay side or a Photo V-Carve: `α`
  from the tool, `start_plane` from the operation, the regions from the
  project (targets nested; an Inlay's near parts merged by
  `dc_geometry::merge_near_parts` at `dc_project::INLAY_MERGE_MM`, #479;
  the male's surround by
  `dc_geometry::surround` on the targets mirrored by
  `Transform::mirror_x_about` the targets' bounding-box centre, closed by
  `R'` from `dc_project::male_band`, and clipped to `MaleBand::board` of
  the male board where it runs past it, #495, #498, all pure geometry,
  never a generator; a Photo's footprint rectangle), and
  `engaged_from` as the generator derives it: the stock top, or the
  start plane when `dc_project::cleared_start_plane` finds an enabled Pocket,
  or an enabled V-Carve's flat floor (#426), at that depth (the pass re-runs that derivation from the project; it is
  geometry on the project, not a generator). For every cutting move of
  the operation in the parsed program:
  - **Tip**: the move's tool-centre path stays in a region: its ends and
    middle inside one or within `tol` of an outline, and it crosses no
    outline segment or arc away from its ends.
  - **Descents** (security review H3; decided in #408): a `G1` with no
    XY motion (within `tol`) that stays above the start plane runs
    through the air over the carve (the start plane is the stock top, or
    cleared) and is held only to the tip rule and S1, S2 and S10. Any
    other descent is held, as a cut, at its lowest point: a cone at one
    XY contains every cone above it there, so that point is all the
    descent cuts. This is stricter than the draft's exemption (a descent
    from the stock top plus the entry clearance that ends no lower than a
    later cut through its XY, checked only for S1, S2 and S10), which
    left the descent's bottom to the later cut; and it accepts a descent
    that starts lower, where S2 lets the rapid stop over an earlier cut
    (#214), as the generator's floor rings under a 0.5 mm flat depth do.
    Every other cutting move is a cut and runs no higher than
    `start_plane + tol`.
  - **Depth range**: `d = start_plane − z ≤ F + tol` with a flat depth;
    with none, the shank bounds it.
  - **Cone**: the disc of radius `d · slope` lies inside a region. The
    test is exact, as S9's (security review M1): for a move at constant Z
    the distance from the path to every outline segment near it, measured
    segment against segment with arcs as arcs
    (`dc_geometry::segment_distance_mm`), is at least `d · slope − tol`;
    for a straight move whose Z varies, the least of `g(t) = dist(p(t),
    wall) − r(t)` with `r(t) = (start_plane − z(t)) · slope` over the
    move is found in closed form for each wall segment and must be at
    least `−tol` (the distance to a line or arc wall splits into the
    distances to its ends and, where the tip is abreast or within the
    sweep, to its line or circle; each less `r` is convex, with a closed
    stationary point, or concave inside a circle, least at an end). An
    arc move whose Z varies (no V toolpath has one) is held to its deeper
    end's radius all along. No sampling step: `MATERIAL_SAMPLE_MM` is
    gone. For a Photo the disc test is replaced by S19's footprint test
    (a groove's width may cross the picture's edge; S7 bounds it).
  - **Shank** (security review H4): `engaged_from − z <= cone + tol`
    for every cut, else `ConeHeightExceeded` (a tip below the cone height
    is this too); for a male Inlay `engaged_from` is always the male
    board's top.
  - **Open band edge** (#448): for a male Inlay whose edge is open, the
    board beyond `B` stands to the male board's top, so the same tests
    hold against `B` with that top as the plane: a V-bit move whose disc
    at the stock top, `(top − z)·s`, crosses `B` fails as
    `ConeOffMaterial`.
  A failure is `ConeOffMaterial` with the overlap and the worst point in
  project space, or `ConeHeightExceeded`. `tol` is S9's slack (#408):
  the program's rounding, plus the flatten tolerance (the generator
  measures its chains and ring links against chords, which near a
  concave arc lie up to that far beyond the true wall, #407; the floor's
  chords where grown islands touch are within it, #405), plus the
  arc-fit tolerance when arcs are fitted (S12), plus `EPS_MM` (the floor
  trace runs exactly at `F · slope` from the walls).
- **S9, clearing runs** (security review M3): a carve's clearing run
  (an `EmitOp` labelled `Clearing(tool)`) is held to the carve's floor
  regions, re-derived in the pass from the carve's targets (or the
  male's surround), the carve's V-bit angle and its flat depth through
  `dc_geometry::floor_regions` (less noise; for a male whose edge is open,
  `surround_floor_regions`, out to `B`, #448), and to the floor's depth
  below the start plane. The reach of the passes before it in the
  carve's `clearing` list (their tools' radii, as `rest_of` is
  re-derived) is what its tool centres must touch, not a bound on its
  footprint: a rest pass's footprint runs into what they cleared (#406).
  S14's rule unchanged.
- **S9, an Inlay cut-out** (#443): a male's run labelled `CutOut(tool)`
  with its cut-out's flat end mill is held to the band's floor rings,
  re-derived from the project with `dc_geometry` and
  `dc_project::male_band` only: the mirrored targets, `surround` at `M`
  closed by `R'`, `surround_floor_regions` at `P·tan(α/2)` (out to `B`
  when the edge is open, #448) less noise, less the floors inside a
  design outline (the design's holes).
  Every cut's tool-centre path lies in a ring at least `r` from its
  outlines, at any Z: above the floor the carve cleared the ring to the
  top; below it the cut-out cuts the board's base, which S1 bounds with
  the cut-out's own `through_cut_allowance_mm` (`Material::floor_mm`).
  And every point of it (its ends and middle, as an Engrave's are held)
  lies on the cut-out's path within the slack (#461): the pass derives
  the path itself, `inset_outers(rounded_band(design, M, R'),
  MaleBand::cut_out_inset_mm(d))`, step 6, on the band clipped to the
  board where it runs past it, and with an open edge less its stretches
  along the board's edge (#495). Clipping never loosens S7: a cut-out
  whose tool passes the board's edge is still refused
  (`safety::cut_out_off_the_stock_still_fails`). Its tabs are held as a
  Profile's (#337): the pass builds the On line Profile itself on that
  path and places its tab request by `dc_cam::placed_tabs::place`, the
  documented layout rule. It never calls a generator
  (`s17_uses_no_generator`).
- **S18, countersink.** For a Drill whose cycle is a countersink: every
  cut (`G1` with Z motion) in its span is vertical, at a target circle's
  centre, from at most the retract plane to exactly `start_plane − D /
  (2 · tan(α/2))` (within rounding); no `G73`/`G81`–`G83` word appears in
  the span; no other motion but rapids at or above the retract plane
  (S15) and `G4`. Else `CountersinkDepth`. As built (#427,
  `material/countersink.rs`), all re-derived from the project:
  - The target circles are the closed profiles a Drill finds, less those
    the mouth rule skips (`D − hole ≤ 1e-9`); the retract plane is
    `min(safe Z, stock top + max(retract_mm, 0))`, as the generator's.
  - "Every cut" is every `G1`, `G2` and `G3`: an arc, or a `G1` whose
    start or end is off a centre, fails. "Within rounding" is one unit
    in the last written place plus `EPS_MM`, in XY and Z alike: the
    centres and the depth are computed from the same values with the
    same functions as the generator's, and a plunge is never flattened
    or fitted.
  - A rapid ends at or above the retract plane; one that starts below it
    (the retract from the bottom) moves in Z alone.
  - The cycle rule refuses a `G73`/`G81`–`G83` word and also any move a
    cycle makes (a modal cycle line has no word), before the motion
    rules: a `G81` to the right depth makes exactly the plain moves.
  - The flank's engagement is held to the cone as S17's shank is
    (`ConeHeightExceeded`): `engaged_from − z ≤ cone + rounding`, with
    `engaged_from` the start plane when it is the stock top or an
    enabled earlier Pocket or V-Carve floor at the start depth holds the
    bit (`dc_project::cleared_start_plane`), else the stock top, and the cone
    `countersink_depth_mm` of the tool's diameter. A mouth wider than the
    bit fails here wherever it starts.
  - `CountersinkDepth`'s `expected_mm` is always the derived bottom and
    `found_mm` where the line ends, project space.
  - The emitter treats a countersink as no Drill when it plans canned
    cycles: it is plain moves by design, so `canned_cycle_expanded` is
    not warned for it.
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
| `vcarve-medial-text-rag` | `v_carve_medial` | outline text "Rag", 10 mm, 60°, `maxCarveDepth` 5 | the resolver's polygons are also written to `tests/fixtures/vcarve/rag.json` for the ported tests. PureCutCNC's outline text uses three.js's Droid typefaces ("Rag" and "g" in Droid Sans Bold, `gA` in Droid Serif Bold Italic), Apache-2.0; its Noto Sans SC (OFL-1.1) serves only its PDF booklets (corrected in #402). The converted glyph data is a modified version: `tests/fixtures/vcarve/README.md` carries the Droid NOTICE and the source commit, and `NOTICE` names it (security review L4). It covers fixture data only, not code, which `cargo deny` does not see |
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
has `diameter` 6; the DreamCarve tool 6.35 mm. As built (#410): cuts at
the flat depth are compared by reach (every golden cut there within
`F · tan(α/2)` of a DreamCarve cut at that depth); a cut off the other
program's path passes only as a link between two points where the other
program lifts and plunges; chain direction is not compared (#404,
decision 3); the offset case compares its rings level by level as sets
and not its links; the `.camj` files turn off PureCutCNC's export arc
fitting, which turns a square ring into a circle past the wall. Three
`Difference`s: `DescentsHeldToPlunge` (S10) on every case;
`ChainDepthFitted` on `vcarve-medial-text-rag`, where upstream's
simplified chords reach up to 0.256 mm past a wall and DreamCarve's depth
fitting (#404, decision 2) raises them; and `EveryCornerReached` on
`vcarve-medial-text-rag` (#456), where upstream's medial axis misses a
corner turning 39.5° by 0.097 mm and five curved stroke ends by 0.010 to
0.030 mm: DreamCarve must come within 0.01 mm of every convex
corner turning more than 20°, and a cut of DreamCarve's on no golden cut
must lie on the V surface and reach a wall the golden program misses by
more than 0.01 mm. The other seven cases are unchanged by #456: upstream
reaches their corners. And `ChainsOnTheAxis` (#457) on every medial
case but the circle (a single plunge): DreamCarve's simplification is
measured on the carved edge and its chains are split onto the medial
axis between nodes, where upstream's chords and its straight cuts into
a junction run off it. Where the programs part, a DreamCarve point must
be on the final V surface (its cone past no wall, and DreamCarve's carve
there as deep as the walls allow, within the tolerance), and a golden
point must be carved at least as deep by DreamCarve or reach past a
wall; DreamCarve's deepest cut above a flat depth may be nearer it than
the golden's (the rectangle's −4.98 against −4.926, where the corner
diagonals run down onto the floor).

**RasterCarve cases** (`tools/parity/photo/`): `capture.py`, run as
`python3 -I capture.py --rastercarve ../../../rastercarve --case
photo-raster-inch`. It invokes the RasterCarve checkout's own virtual
environment interpreter by absolute path with an argument list (no
shell), as `python -m rastercarve <args> <fixture>`, from a fresh
temporary working directory, never adds the checkout to its own
`sys.path` and imports nothing from it; `--case` must match
`^[a-z0-9-]+$` before `tests/golden/<case>/` is built; it writes only
`program.nc`, as RasterCarve wrote it, and a README with the command
line, RasterCarve's commit, its statistics block and the frame mapping.
The committed `program.nc` is a GPL program's output on DreamCarve's own
fixture, which the GPL does not cover, and carries no RasterCarve source
text; nothing else of RasterCarve's is read, imported or written
(security review M5). The draft's `photo-hatch` case, which would have
driven RasterCarve's Python engine from a script, is dropped: that is a
combined work. Hatch is covered by the unit tests alone.

| Case | Picture | Arguments | DreamCarve |
| --- | --- | --- | --- |
| `photo-raster-inch` | `ramp.png` (64 × 32, black left to white right, linear) | `--width 0.1` (defaults: 30°, 0.08 in, 22.5°, 110 %, 0.01 in, safe 0.1, feeds 100/30) | 2.54 mm wide at the origin, `max_depth_mm` 2.032, `sample_mm` 0.254, the same bit and angle; the harness scales the golden by 25.4 |
| `photo-raster-metric` | `radial.png` (64 × 64, white centre to black edge, radial) | `--metric --width 100 -t 30 -d 2 -a 22.5 -s 110 -r 0.25 -z 5 -f 1000 -p 300` | as written |

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
crates/dc-geometry/src/{medial.rs (medial axis, resolution, convex corners, point_in_loops), medial/{grid.rs (SegmentDistanceGrid), tests.rs}, carve.rs (floor_regions, surround, rounded_band, surround_floor_regions, inset_outers, is_design_hole), merge.rs (merge_near_parts, MergedParts, NearParts; #479), transform.rs (+ mirror_x_about)}
crates/dc-project/src/{model.rs (+ VCarveOp, VCarveStrategy, InlayOp, MaleBoard, Board, PhotoVCarveOp, PhotoStrategy, Bitmap, DrillCycle::Countersink, ClearingPass, Project::bitmaps), bitmap.rs (Bitmap::pixels, Grayscale), ids.rs (+ BitmapId), migrate.rs (v19_to_v20 … v25_to_v26, MigrationNote, migrate_with_notes), project.rs (cleared_start_plane, ClearedPlane), inlay.rs (male_band, edge_pass, MaleBand, BandEdge; #448; INLAY_MERGE_MM; #479; Project::on_male_board, Project::male_board; #498), validate.rs (+ rules above), serialize.rs (png_base64 on one line)}
crates/dc-cam/src/{vcarve.rs (Carve, blocks, cone_height_mm, cone_in_material), vcarve/{medial.rs (chains, pruning, simplify, blocks, links), offset.rs, floor.rs, clearing.rs (ToolRun per pass)}, inlay.rs, photo.rs, drill.rs (+ countersink), pocket.rs (+ pocket_regions, the entry point on resolved regions), pocket/rest.rs (reach shared with clearing; left_by takes several references), order.rs (+ start_depth_mismatch, start_depth_uncleared, calling dc_project::cleared_start_plane), block.rs (+ entry_z_mm), sequence.rs (+ stay_down_links, entry_z_mm), test_support.rs (+ assert_cone_in_material), warning.rs, error.rs}
crates/dc-gcode/src/{material.rs (+ S17 exact cone and shank, S18, S19, clearing-run floor), error.rs (+ variants), safety.rs (dispatch, descent exemption)}
crates/dc-import/src/{bitmap.rs, bitmap/{png.rs, jpeg.rs, gray.rs}}
crates/dreamcarve/src/{init.rs (+ pictures), image.rs (image add/list), cli.rs (+ Image), export.rs (+ dispatch of the three kinds, tool runs, the summaries, programs by board and `--board`), preview.rs (+ <image>, the male board)}
tools/parity/{cases/vcarve-*.camj, cases/vcarve-*.notes.md, photo/{capture.py, README.md}}
tests/golden/{vcarve-medial-rect, vcarve-medial-square, vcarve-medial-circle, vcarve-medial-ring, vcarve-medial-l, vcarve-medial-text-rag, vcarve-medial-triangle, vcarve-offset-square, photo-raster-inch, photo-raster-metric}/
tests/fixtures/{sign.svg, dc-inlay.svg, dc.dcv, photo/{ramp.png, radial.png, gradient.png}, vcarve/{README.md (font notice), rag.json, g.json, ga-small.json, ga-large.json}}
crates/dc-project/tests/fixtures/dcv/{v19,v20,v21,v22,v23,v24,v25,v26}/sample.dcv
crates/dreamcarve/tests/{fixtures,expected}/inlay-migration/ (the two-file pairs of formats 23 and 24 and their programs, #498)
```

New dependencies, all `[workspace.dependencies]` with
`default-features = false`:

- `delaunator` 1 (ISC): Delaunay triangulation, the Rust port of the
  library PureCutCNC uses, so the medial axis triangulates as upstream
  does. One dependency, `robust` (Apache-2.0 OR MIT), the exact
  orientation and in-circle predicates that make the triangulation
  deterministic across platforms. `dc-geometry`.
- `png` 0.18 (MIT/Apache-2.0): PNG decode and encode, with
  `default-features = false` and the `flate2` backend already in the
  tree (`zlib-rs`), so the workspace carries one inflate implementation.
  Pulls `fdeflate`, `crc32fast`, `bitflags`. `dc-import` (decode any
  PNG, encode the stored one), `dc-project` (decode the stored one).
  Versions are re-checked against crates.io at slice time (the lockfile
  already holds `base64` 0.22; the newest at writing is 0.23).
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
- `migrate::v19_to_v22_steps_bump_the_version_only`, `migrate::v22_project_gains_an_empty_bitmap_list`, `migrate::v22_with_bitmaps_already_present_fails`, `migrate::version_19_to_22_fixtures_load`, `cli::v19_project_exports_the_same_program`.
- `serialize::v_carve_inlay_photo_round_trip`, `serialize::v_carve_strategy_is_string_or_object`, `serialize::countersink_cycle_round_trips`, `serialize::bitmap_png_is_one_line`, `bitmap::pixels_decode_the_stored_png`, `bitmap::wrong_size_png_is_an_error`, `bitmap::colour_png_is_an_error`.
- `validate::v_carve_with_end_mill_is_error`, `validate::v_bit_angle_out_of_range_is_error`, `validate::flat_depth_beyond_cone_is_error`, `validate::photo_max_depth_beyond_cone_is_error`, `validate::countersink_depth_beyond_cone_is_error`, `validate::clearing_needs_a_flat_depth`, `validate::clearing_pass_needs_a_flat_end_mill`, `validate::clearing_wider_than_the_pass_before_warns`, `validate::clearing_pass_naming_no_tool_is_error`, `serialize::clearing_passes_round_trip`, `validate::countersink_mouth_wider_than_tool_is_error`, `validate::negative_start_depth_is_error`, `validate::inlay_glue_gap_must_be_below_depth`, `validate::inlay_male_needs_margin`, `validate::thin_plug_base_warns`, `validate::photo_angle_and_spacing_ranges`, `validate::photo_spacing_below_one_warns`, `validate::photo_names_a_missing_bitmap`, `validate::bitmap_aspect_must_match_pixels`, `validate::countersink_drill_with_v_bit_is_not_unusual`.
- Security review additions: `validate::uncleared_start_depth_is_an_error_unless_acknowledged` (M2: V-Carve, Photo and countersink; a containing Pocket at the depth is quiet; a disabled one does not count), `validate::male_inlay_engagement_beyond_the_cone_is_error` (H4: `D 2.5, G 0.25, P 1.5` with a 90° 6.35 mm bit fails; the star's `4, 1, 2` with the 60° bit passes), `validate::acknowledged_start_depth_counts_toward_the_cone` (H4), `validate::flatten_tolerance_above_0_1_mm_is_error` (L6), `bitmap::empty_or_oversized_bitmap_is_refused_from_the_header` (M4: a 0 × 0 and a 9000 × 10 PNG are refused without decoding image data), `bitmap::decompression_bomb_is_refused_by_the_decoder_limit` (M4: a 1 MB base64 string holding an 8192 × 8192 stream fails under the limit, not out of memory), `bitmap::header_lying_about_size_is_refused` (M4), `validate::bitmap_footprint_must_be_positive_and_on_the_coordinate_range` (M4).

**dc-cam — vcarve (medial)**
- `vcarve::square_clamps_at_flat_depth_and_surfaces_at_corners` (port: `testGeneratorSquare`: 20 × 20, 60°, flat 2: every cut inside the square, min Z −2 ± 1e-6, max Z ≥ −1e-6, no warnings, continuity).
- `vcarve::depth_follows_the_bit_angle` (port: `testGeneratorDepthMatchesVBitGeometry`: 30 × 5 bar, 60° → −4.330 ± 0.02, 90° → −2.5 ± 0.02).
- `vcarve::end_mill_is_refused` (port: `testGeneratorRequiresVBit`), `vcarve::same_input_twice_is_identical` (port: `testGeneratorDeterminism`), `vcarve::two_bars_are_never_bridged` (port: `testGeneratorMultipleFeatures`: no cut has both ends in the 10 mm gap), `vcarve::nearest_region_is_cut_first` (port: `testGeneratorVisitsNearestResolvedRegionFirst`), `vcarve::outline_text_target_carves` (port: `testGeneratorTextFeatureTarget`, `rag.json`), `vcarve::dot_region_is_one_plunge_and_no_cut` (port: `testDotRegionCollapsesToSinglePlunge`), `vcarve::nearby_chain_ends_link_tool_down_within_the_carved_disc` (port: `testEmissionLinksNearbyChainEnds`: one plunge and three cuts at clearance 1; two plunges at 0.05), `vcarve::links_use_the_clamped_disc` (port: `testLinkRespectsClampedDepth`), `vcarve::triangle_arms_surface_at_three_corners` (port: `testGeneratorTriangle`).
- `vcarve::rectangle_centreline_depth_is_half_width_over_tan` (acceptance 48: 20 × 5, 60° 6.35 mm: centreline −4.330 ± 0.01, every cutting point's depth equals its wall distance / tan 30° ± 0.02, each corner reached at Z ≥ −0.01), `vcarve::six_mm_square_with_90_degree_bit_meets_at_three_mm`, `vcarve::six_mm_circle_is_one_plunge_at_5_196`.
- `vcarve::ring_runs_along_its_centreline_and_rises_at_outer_corners` (acceptance 49: 24 mm square with 12 mm hole, 90°), `vcarve::no_cut_is_nearer_a_wall_than_its_depth_allows` (the cone test on every fixture), `vcarve::triangle_meets_at_the_incentre`.
- `vcarve::open_path_is_skipped_with_warning`, `vcarve::cone_exceeded_is_an_error_naming_the_depth` (acceptance 52), `vcarve::flat_depth_within_the_cone_exports`.
- `vcarve::start_depth_lowers_every_cut_and_plunges_from_the_stock_top` (acceptance 53), `vcarve::one_pass_whatever_the_depth`.
- `vcarve::chains_become_blocks_that_sequence_in_generator_order`, `vcarve::both_machining_orders_cut_the_same` (one pass).
- Security review additions: `sequence::a_plunge_ends_at_the_blocks_entry_z_not_its_deepest_z` and `sequence::validate_block_rejects_an_entry_below_the_deepest_z` (H1), `vcarve::corner_tip_chain_plunges_to_the_surface_not_the_centreline` (H1: the 20 × 5 mm rectangle's first plunge ends at Z 0 ± 0.01 at a corner), `vcarve::links_never_run_below_the_final_v_surface` (H2: for every stay-down link, at every point along it the clearance is at least the interpolated `d · slope`; the `testEmissionLinksNearbyChainEnds` graph at clearance 1 now retracts where upstream bridged, and the test says so), `vcarve::offset_ring_links_pass_the_cone_test_or_retract` (H2), `vcarve::uncleared_start_depth_is_refused_and_acknowledged_counts_toward_the_cone` (H4, M2: `start_depth_mm: 4` with the 90° bit is `VBitConeExceeded` even for a 1 mm carve; in a 4 mm Pocket it exports), `vcarve::generator_refuses_its_own_toolpath_on_a_cone_violation` (step 10, on a hand-built `Carve` whose region is narrower than the chain's disc).

**dc-cam — vcarve (flat floor and clearing)**
- `vcarve::floor_boundary_is_traced_at_the_flat_depth` (acceptance 50: 30 × 10, 90° 6.35 mm, flat 2: nothing below −2; the 26 × 6 boundary traced at −2 within 0.01; bisector runs from each floor corner to the surface corner).
- `vcarve::v_bit_floor_covers_the_floor_region` (union of 4 mm footprints at −2 covers the floor within 0.05 at `step_over_fraction` 0.4 and 0.1), `vcarve::floor_ridge_height_is_reported` (0.8 mm at 0.4 and 0.2 mm at 0.1; no higher across the floor's middle; none without a floor), `vcarve::corner_ridge_is_reported_at_the_rings_sharpest_corner` (#522: rectangle 0.2 / sin 45°, pentagon 0.2 / sin 54°, a 30° corner the cap 0.4, a circle 0.2; the floor no higher than reported), `vcarve::floor_without_a_step_over_is_refused` (#405: `MissingStepOver`, `InvalidStepOver` at 1.5; none needed without a floor), `cli::v_carve_check_reports_the_floor_ridges`.
- `vcarve::chains_inside_the_cleared_floor_are_dropped`, `vcarve::chains_reaching_corners_are_kept`.
- `vcarve::clearing_pass_takes_the_floor_region_and_depth` (acceptance 51: the run's footprint inside 26 × 6, depth 2), `vcarve::clearing_runs_come_before_the_v_bit_run_with_their_own_tools`, `vcarve::floor_cutting_shrinks_to_what_the_end_mill_cannot_reach` (within 1.5875 of the boundary; under a third of the no-clearing length), `vcarve::floor_coverage_with_the_pass_still_reaches_the_floor`, `vcarve::second_pass_clears_only_what_the_first_left`, `vcarve::wider_second_pass_warns_not_descending_and_still_runs`, `vcarve::clearing_without_a_flat_depth_is_an_error`, `vcarve::clearing_pass_with_a_v_bit_is_an_error`, `vcarve::removing_the_list_gives_the_v_bit_the_whole_floor`, `vcarve::pass_that_reaches_nothing_warns_and_emits_no_run`, `vcarve::clearing_run_footprint_stays_inside_the_floor` (S9/S14), `vcarve::clearing_pass_is_cut_by_the_pocket_code` (the run equals a user Pocket of the floor region move for move), `emit::clearing_runs_become_programs_of_their_own_tools` (`op2 (clearing t1)` in the program comment and the summary), `order::clearing_runs_count_for_the_tool_change_hint`, `cli::preview_legend_names_clearing_runs`.

**dc-cam — vcarve (offset)**
- `vcarve::offset_rings_are_inset_by_their_depth` (acceptance 54: 6 mm square, 0.5 mm steps, 90°: insets 0.5…2.5 at Z −0.5…−2.5 ± 0.01), `vcarve::offset_climb_and_conventional_reverse_every_ring`, `vcarve::offset_stops_at_the_flat_depth_and_clears_the_floor`, `vcarve::offset_ring_links_pass_the_cone_test_or_retract` (H2), `vcarve::offset_ring_direction_matches_pocket_rings` (Y-up re-derivation note).

**dc-cam — drill (countersink)**
- `drill::countersink_depth_is_mouth_over_two_tan_half_angle` (port: "90° V-bit: a 6 mm mouth plunges 3 mm" and "82° V-bit: depth follows D / (2·tan(θ/2))": −3 and −3.451), `drill::countersink_depth_is_measured_from_the_start_plane` (port: "depth is measured from each target's own top face", restated: start depth 4 → Z −7), `drill::countersink_is_a_centre_plunge_and_retract` (port: "motion is centre-plunge and retract"), `drill::countersink_needs_a_v_bit` (port), `drill::countersink_rejects_an_unusable_angle` (port), `drill::countersink_rejects_a_missing_or_zero_diameter` (port), `drill::countersink_mouth_wider_than_the_cutter_is_refused` (port), `drill::full_diameter_countersink_is_cut_within_one_ulp` (port: "a tool limited to exactly its cone height still cuts"), `drill::mouth_no_wider_than_the_hole_skips_that_target` (port), `drill::non_circular_targets_are_skipped` (port), `drill::countersink_records_no_canned_cycle` (port: "countersink posts as linear motion even on a canned-cycle machine", the generator half), `drill::other_cycles_are_unchanged_by_the_new_variant` (port: "legacy drilling operations are untouched"), `drill::countersink_dwell_is_at_the_bottom`, `drill::countersink_rapid_stops_above_the_stock_top_whatever_the_start_depth` (H3: `start_depth_mm: 4`, `retract_mm: 1` rapids to the stock top + 1 and plunges from there), `drill::uncleared_countersink_start_depth_is_refused` (M2, H4).

**dc-cam — inlay**
- `inlay::female_equals_a_flat_depth_v_carve` (acceptance 56, move for move).
- `inlay::male_engagement_beyond_the_cone_is_an_error` (H4: `D 2.5, G 0.25, P 1.5`, 90° 6.35 mm bit → `VBitConeExceeded` naming 3.75 mm and 3.175 mm; the star's values pass), `inlay::male_shank_check_in_the_safety_pass_counts_from_the_stock_top` (H4, `safety::`).
- `inlay::male_mirrors_about_the_targets_centre_line` (acceptance 57: the mirrored outlines; the note's X), `inlay::male_carves_the_surround_and_the_holes` (every cutting point in the band or the circle), `inlay::male_floor_is_at_depth_minus_gap_plus_plug` (nothing below −5), `inlay::male_flank_passes_through_the_outline_at_the_start_plane` (within 0.02), `inlay::female_and_male_walls_mate` (the fit property, `z` 0..3, within 0.02), `inlay::glue_gap_at_or_above_depth_is_an_error`, `inlay::thin_plug_base_warns_on_10_mm_stock_and_not_on_12`, `inlay::clearing_pass_clears_the_male_floor`, `inlay::surround_of_a_star_with_a_hole_has_the_hole_as_a_region`, `inlay::mirror_negates_bulges_and_keeps_areas` (arcs survive the mirror).
- The cut-out (#443): `inlay::cut_out_runs_last_with_its_own_tool`, `inlay::cut_out_stays_inside_the_floor_ring` (the footprint at least `r` from both edges), `inlay::cut_out_starts_at_the_floor_and_goes_through` (no lap above the floor; the last at −allowance), `inlay::cut_out_skips_design_holes`, `inlay::cut_out_leaves_the_tabs`, `inlay::cut_out_ignored_by_the_female_and_refused_on_a_narrow_floor` (#498: the female ignores the cut-out); `validate::cut_out_no_longer_needs_a_side` (#498; was `validate::cut_out_on_a_female_is_an_error`), `validate::cut_out_needs_a_flat_end_mill`, `validate::cut_out_with_no_room_in_the_floor_ring_is_an_error` (the star, `margin_mm: 4`, 3.175 mm), `validate::cut_out_tool_needs_its_rates`; `migrate::v22_to_v23_adds_a_null_cut_out`, `migrate::version_22_fixture_loads`, `serialize::inlay_cut_out_round_trips`.
- The open, rounded band (#448): `carve::rounding_zero_is_the_plain_surround`, `carve::rounded_band_contains_the_plain_band`, `carve::rounding_fills_a_gap_narrower_than_twice_the_radius`, `carve::rounding_keeps_design_holes_as_regions`, `carve::open_floor_keeps_the_band_edge`, `carve::inset_outers_rounds_across_a_gap` (dc-geometry); `migrate::v23_to_v24_adds_null_outline_rounding`, `migrate::version_23_fixture_loads`, `serialize::inlay_outline_rounding_round_trips`, `validate::outline_rounding_must_be_positive`, `validate::cut_out_room_follows_the_open_edge`, `inlay::edge_pass_is_the_first_clearing_pass_that_fits`, `inlay::open_edge_needs_the_pass_and_the_flank_inside_the_band` (dc-project); `inlay::male_with_clearing_has_no_v_wall_at_the_band_edge` (no V-bit point within its cone's radius at the stock top of `B`, nor nearer than `M − P·s`; without clearing, the V wall's foot is cut), `inlay::edge_pass_sweeps_the_band_to_its_outer_edge` (every vertex of `B` is `r` from the floor path within the tolerance), `inlay::male_without_clearing_keeps_the_v_wall`, `inlay::clearing_that_does_not_fit_keeps_the_v_wall_and_warns`, `inlay::female_and_male_walls_mate` (also with `R` 12 and an open edge), `inlay::cut_out_no_room`.
- The cut-out along the floor's outer edge (#461): `inlay::cut_out_hugs_the_band_edge` (open edge, plain and `R` 6: every cut point `r` from `B` within the tolerance and inside it; above the floor the swept disc inside `B`), `inlay::cut_out_hugs_the_slope_foot_with_a_v_wall` (every point `P·s + r` from `B`), `inlay::cut_out_no_room`, `inlay::open_edge_needs_the_pass_and_the_flank_inside_the_band` (`MaleBand::cut_out_inset_mm`), `carve::inset_outers_runs_inside_the_band_edge` (dc-geometry); `safety::cut_out_off_the_floor_ring_fails` and `safety::cut_out_off_the_open_edge_ring_fails` (on the path passes; in the ring off it, where the cut-out ran before, fails), `safety::cut_out_plunge_into_a_tab_fails`, `safety::generated_cut_outs_pass`; `cli::lettered_inlay_fits_its_board` (the tutorial's cut-out `r` from `B`, inside it); `inlay::plug_seats_flush` unchanged.
- The band and the cut-out clipped to the stock (#495): `inlay::band_is_clipped_to_the_stock` (a star 2 mm from the left edge, `M` 8, a 3.175 mm 60° V-bit and a 1.5 mm end mill: no run's tool past the stock, the clearing floor out to the board's edge, `inlay_band_clipped` naming the left edge and 2 mm), `inlay::v_wall_band_is_clipped_inside_the_board` (clipped `R_v` inside it), `inlay::cut_out_runs_edge_to_edge_when_clipped` (open paths, their ends `r` inside the edge, every cut on them and on the stock, the tabs on them), `inlay::clipped_male_still_seats` (`plug_seats_flush`'s 0.05 mm), `inlay::design_too_near_the_edge_is_explained` (the bench's bits: `InlayTooNearEdge`, 4.330 mm needed), `inlay::unclipped_band_is_unchanged` (the tutorial's "DC" and the star: the surround and `B` as before, the same runs on stock ten times the size), `carve::band_on_board_clips_only_what_runs_past`, `carve::without_rect_sides_opens_outlines_along_the_board` (dc-geometry), `inlay::band_is_clipped_to_the_board` (dc-project); `safety::clipped_band_is_derived_from_the_project` (S17 and S9 refuse past the clipped band on 60 mm stock and pass on 100 mm), `safety::cut_out_off_the_stock_still_fails` (the unclipped cut-out on the narrow stock fails S7; a cut along the dropped stretch fails S9).
- Near parts merged (#479): `inlay::near::male_of_parts_closer_than_the_written_precision_seats` (squares 0.001 mm apart), `inlay::near::male_of_a_pinch_seats` (a circle 0.005 mm from a square, 60° and 90°), `inlay::near::male_of_a_merged_pinch_seats_at_every_gap` (0.01, 0.012, 0.02 and 0.04 mm, 60°), `inlay::near::male_of_a_corner_beside_a_curve_seats` (0.016 and 0.03 mm), `inlay::near::male_of_parts_a_hair_apart_seats` (0.003 mm), `inlay::near::parts_closer_than_the_merge_distance_merge_and_warn` (one region a side, one `inlay_parts_merged` a side, the female's pocket across the gap), `inlay::near::parts_a_tenth_apart_are_not_merged` (0.1 mm: two regions, no warning, the female's ridge stands to the stock top, the pair seats); `merge::squares_a_hair_apart_merge`, `merge::circle_at_a_pinch_merges`, `merge::merging_ends_a_gap_in_a_corner`, `merge::island_near_its_hole_merges_and_a_thin_web_does_not`, `merge::touching_parts_and_no_distance_merge_nothing`, `merge::merging_keeps_inward_corners_away_from_the_gap` (dc-geometry); `safety::female_holds_merged_parts_as_one_shape` (a cut over the 0.001 mm gap passes, over a 0.1 mm gap fails S17), `safety::generated_near_parts_pass_the_safety_pass`; the stress tests' `known_issue` no longer excuses near touches.
- One-file Inlay (#498): `migrate::v24_to_v25_female_gets_an_unset_male_board`, `migrate::v24_to_v25_male_gets_the_stock_as_its_male_board`, `migrate::version_24_fixture_loads`, `serialize::inlay_male_board_round_trips`, `serialize::photo_v_carve_is_format_26` (pins the chain the 3.4 slice renumbers); `validate::male_board_size_must_be_positive`, `validate::male_board_thickness_must_be_positive`, `validate::cut_out_no_longer_needs_a_side`, `validate::inlays_share_one_male_board`; `inlay::both_boards_from_one_operation` (the female's runs, then the male's, each tagged), `inlay::male_runs_wait_for_the_board_thickness` (unset: no male runs, `inlay_male_board_unset` at `operations[i].male_board.thickness_mm`), `inlay::male_board_defaults_to_the_stock_in_xy`, `inlay::plug_base_thin_reads_the_male_board` (stock 18 mm, male board 8 mm with `D 4, G 1, P 2`: warns); `inlay::female_and_male_walls_mate` and `inlay::plug_seats_flush` unchanged in meaning, now from one project; `safety::male_program_is_held_to_the_male_boards_bottom` (male board 8 mm: a cut-out through 8.2 mm passes, through 12 mm fails S1), `safety::male_program_stays_on_the_male_board` (S7 against the male board's XY), `safety::generated_inlays_pass_on_both_boards`, `safety::origin_preset_applies_to_the_male_board` (centre origin: the male programs' XY are about the male board's centre); `cli::v24_male_project_exports_the_same_male_programs` (the old `star-male.dcv` and `dc-male.dcv`, migrated, export the committed male programs byte for byte under the new names), `cli::v24_female_project_exports_the_same_female_programs`, `cli::export::programs_are_grouped_by_board` (names, order, `k` restarting per board), `cli::export::board_flag_exports_one_board`, `cli::export::two_inlays_share_one_male_board`, `cli::export::no_inlay_keeps_todays_names`, `cli::preview::male_board_drawn_beside_the_stock`, `cli::lettered_inlay_exports_both_boards`, `quick_start::inlay_pair_chapter_runs` (one file). The stress harness (`crates/dc-gcode/tests/stress`) builds one-project Inlays: the female case with an unset male board, each male case cutting both boards from its one Inlay on a male board the stock's size, its thickness drawn from 10–14 mm on a separate RNG stream, and seating its own two boards.

**dc-cam — order**
- `order::start_depth_matching_an_earlier_pocket_is_quiet`, `order::start_depth_without_a_pocket_is_an_error_unless_acknowledged` (M2; was `…_warns`), `order::acknowledged_start_depth_is_quiet`, `order::start_depth_with_a_shallower_pocket_names_it` (a warning), `order::disabled_pocket_does_not_count`, `order::countersink_start_depth_is_checked_too`, `order::photo_footprint_is_checked_too`, `order::clearing_pockets_are_not_containing_pockets`, `order::inlay_is_exempt`, `order::validation_and_check_agree_on_the_containing_pocket`, `order::countersink_on_a_v_carve_floor_at_another_depth_warns_mismatch` (#426).
- A V-Carve floor as the start plane (#426): `project::v_carve_floor_clears_its_start_plane` (a hole inside the floor region at `s0 + F` is cleared; one inside the targets but on the slope is not), `project::disabled_or_later_v_carve_does_not_clear`, `project::v_carve_without_a_flat_depth_does_not_clear`, `drill::countersink_on_a_v_carve_floor_engages_only_its_own_depth`, `validate::countersink_on_a_v_carve_floor_is_not_uncleared`.

**dc-import — bitmap**
- `bitmap::png_gray_8_reads_as_is`, `bitmap::png_rgb_converts_by_luminance_and_warns`, `bitmap::png_rgba_flattens_on_white`, `bitmap::png_16_bit_takes_the_high_byte`, `bitmap::png_palette_expands`, `bitmap::jpeg_reads_and_converts`, `bitmap::extension_and_magic_must_agree`, `bitmap::too_large_is_refused`, `bitmap::encode_is_deterministic`, `bitmap::encode_then_decode_round_trips`.

**dc-cam — photo**
- `photo::move_budget_refuses_a_microscopic_sample_step` (L3), `photo::link_height_clears_the_stock_top_when_the_start_plane_is_only_vouched_for` (L1), `photo::uncleared_start_depth_is_refused` (M2).
- `photo::grooves_are_spaced_by_the_fraction_of_the_full_depth_width` (acceptance 59: 1.179 ± 0.001 at 30°, 2 mm, 1.1), `photo::grooves_run_at_the_line_angle_and_alternate`, `photo::one_groove_passes_through_the_top_left_corner`, `photo::zero_angle_grooves_are_parallel_to_x`, `photo::every_cut_is_inside_the_footprint`, `photo::black_is_max_depth_white_is_the_surface_and_grey_is_between` (`gradient.png` with its blocks: −2.000, 0.000 ± 0.01; 50 % ± 0.02), `photo::invert_swaps_black_and_white`, `photo::hatch_adds_a_second_family_at_ninety_degrees`, `photo::between_grooves_the_bit_rises_to_the_retract_and_feeds_across`, `photo::spacing_below_one_warns`, `photo::collinear_samples_merge_into_one_move`, `photo::sample_is_bilinear_and_clamped_at_the_edges`, `photo::top_row_is_at_the_highest_y`, `photo::max_depth_beyond_the_cone_is_an_error`, `photo::start_depth_lowers_the_grooves`, `photo::unknown_bitmap_is_an_error`, `photo::same_input_twice_is_identical`.

**dc-gcode — safety**
- `safety::cone_through_a_wall_fails_s17` (a hand-built toolpath whose disc crosses the outline), `safety::tip_below_the_flat_depth_fails_s17`, `safety::tip_below_the_cone_height_fails_s17`, `safety::generated_v_carves_pass_s17` (every vcarve fixture), `safety::generated_inlays_pass_s17`, `safety::male_surround_is_derived_from_the_mirrored_targets`, `safety::cut_out_off_the_floor_ring_fails`, `safety::generated_cut_outs_pass`, `safety::cut_out_plunge_into_a_tab_fails`, `safety::cut_out_is_held_to_its_own_allowance` (#443), `safety::male_band_rounding_is_derived_from_the_project`, `safety::v_bit_across_the_open_edge_fails_s17`, `safety::cut_out_off_the_open_edge_ring_fails`, and the generated tests with open, rounded males (#448).
- Security review additions: `safety::vertical_descent_above_the_start_plane_passes_s17` and `safety::a_sideways_cut_above_the_start_plane_fails_s17` (H3), `safety::cone_test_is_exact_between_vertices` (M1: a move passing a convex corner with its closest approach between what 1 mm samples would have read fails; a crossing of a 0.5 mm island fails), `safety::constant_z_rings_take_the_segment_path` (M1), `safety::shank_engagement_from_the_stock_top_fails_s17` (H4: a `start_depth_mm: 4` carve acknowledged with the 90° bit), `safety::shank_engagement_from_a_cleared_start_plane_passes` (H4), `safety::clearing_run_is_held_to_the_rederived_floor` (M3), `safety::s17_uses_no_generator` (a compile-time check: `dc-gcode` imports only `dc_cam::{Toolpath, Move, MoveKind, DrillHole, …}` types; the test greps the crate's `use` lines).
- `safety::countersink_at_the_wrong_depth_fails_s18`, `safety::countersink_written_as_g81_fails_s18`, `safety::generated_countersinks_pass_s18`.
- `safety::groove_outside_the_picture_fails_s19`, `safety::groove_link_as_a_rapid_fails_s19`, `safety::groove_below_max_depth_fails_s19`, `safety::generated_photos_pass_s19`, `safety::edge_groove_width_is_s7s_business` (a full-depth groove at the picture's edge passes S19 and fails S7 only when it leaves the stock).
- `safety::s17_check_is_fast_on_the_sign` (the sign fixture's two carves under 2 s, as `s9_check_is_fast_*`).

**dreamcarve (integration, `tests/`)**
- `cli::init_on_a_picture_writes_a_bitmap_and_a_photo_v_carve` (acceptance 58: `--width 100` → 100 × 50; `--height 50` the same; both or neither exit 2), `cli::image_add_places_a_picture_and_image_list_shows_it`, `cli::image_add_converts_colour_and_says_so`, `cli::moved_dcv_exports_the_same_program`, `cli::picture_past_the_stock_edge_fails_s7`.
- `cli::check_prints_the_carve_summary_and_the_mirror_note`, `cli::check_warns_start_depth_mismatch`, `cli::preview_draws_the_bitmap_under_the_grooves`.
- `cli::sign_exports_four_programs_in_tool_run_order` (acceptance 61: `fixtures/sign.svg`; every program passes S1–S19; the preview has the floor rings, the serif lines, two countersinks and the tabbed outline), `cli::lettered_inlay_exports_both_boards` (#446, #498; was `cli::lettered_inlay_female_and_male_export`: the one project `fixtures/dc.dcv` from `fixtures/dc-inlay.svg` exports `dc-female-1-T2.nc` and `-2-T1` (T2 clearing, T1 V-bit) and `dc-male-1-T2.nc`, `-2-T1` and `-3-T2` (T2 clearing, T1 V-bit, T2 cut-out), byte for byte the programs the old two-file pair exported; all pass S1–S18; the mirror note is printed), `cli::lettered_inlay_fits_its_board` (every cut inside the 110 × 80 mm board with at least 3 mm to spare; the cut-out `margin_mm / 2` out from the mirrored letters), `cli::lettered_male_is_the_mirror_of_the_female` (the male's design outlines are the female's mirrored about the stated X, within the flatten tolerance), `cli::v22_project_exports_the_same_program`, `cli::photo_quick_start_runs` (the chapter's commands).
- `quick_start::v_carved_sign_chapter_runs` (as the phase 2 plaque's).

**dc-gcode (integration, `tests/golden.rs`)**
- `golden::vcarve_medial_rect`, `golden::vcarve_medial_square`, `golden::vcarve_medial_circle`, `golden::vcarve_medial_ring`, `golden::vcarve_medial_l`, `golden::vcarve_medial_text_rag`, `golden::vcarve_medial_triangle` (in order per region, chains as sets, 0.02 mm), `golden::vcarve_offset_square` (rings as sets per level, 0.02 mm).
- `golden::photo_raster_inch`, `golden::photo_raster_metric` (groove for groove, 0.01 mm XY, 0.05 mm Z, after the frame map and, for the inch case, × 25.4).
- `golden::phase_1_and_2_cases_are_unchanged` (every earlier golden still passes: the `sequence` change for Photo links and the 3D block Z reading touch shared code).

## Sub-phases

The phase is delivered as five sub-phases, one feature at a time, so each
can be followed, tested and iterated on its own (decided 2026-10-28). Each
is a GitHub milestone (`3.1 V-Carve`, …), lands its own format version,
and ends with a bench check. Slices follow phase 2's practice: one issue
and one PR each, with a Tests section drawn from the test plan;
dependencies in brackets. No tag between sub-phases (`docs/plan.md`,
"Versioning"); `v0.3.0` comes at 3.5.

### 3.1 V-Carve (format v20)

Goal: the sign carved on the bench without its countersinks.

1. **Format v20: V-Carve.** `VCarveOp`, `VCarveStrategy`, `ClearingPass`,
   `v19_to_v20`, the V-Carve and clearing validation rules including
   `start_depth_uncleared` and the engagement sum. Tests: the dc-project
   V-Carve, clearing and migration tests. [none]
2. **Medial axis geometry.** `dc-geometry::medial` on `delaunator`, the
   ported geometry and resolution tests, `dc-geometry::carve::floor_regions`,
   the text fixtures captured as polygon JSON by the parity tool with
   their font-notice README. Tests: the dc-geometry medial group. [none]
3. **Blocks with an entry Z.** `CutBlock::entry_z_mm`, the `sequence`
   change, `validate_block`; the phase 1 and 2 generators set it; every
   earlier golden unchanged. Tests: the H1 sequence tests,
   `golden::phase_1_and_2_cases_are_unchanged`. [none]
4. **V-Carve, medial strategy.** `dc-cam::vcarve` with `Carve`, `ToolRun`,
   chains, pruning, simplification, blocks, cone-tested links,
   `cone_in_material` and the generator's own S17 check, the start plane
   with `dc_project::containing_pocket` (now `cleared_start_plane`), `check`/`export` dispatch and summary
   (one run per operation). Clamping only, as PureCutCNC. Tests: the
   vcarve (medial) group with its H1, H2 and H4 additions. [1, 2, 3]
5. **Flat floor and the V-bit floor.** The boundary trace, the ridge
   rings, chain dropping. Tests: `vcarve::floor_*`, `vcarve::v_bit_floor_*`,
   `vcarve::chains_*`. [4]
6. **Clearing passes.** `vcarve::clearing`, the Pocket entry point on
   resolved regions, tool runs through `export`, `order::check` and the
   preview. Tests: the clearing bullet of the vcarve (flat floor and
   clearing) group. [5]
7. **Offset strategy.** Tests: the vcarve (offset) group. [4]
8. **S17 in the safety pass.** `material.rs` exact cone and shank checks
   with the descent exemption, the clearing-run floor, the error
   variants, the no-generator import test. Tests: the safety S17 tests
   and their security-review additions, V-Carve and clearing cases. [5, 6]
9. **Start-depth checks.** `order::check`'s `start_depth_mismatch` and
   `start_depth_uncleared` for V-Carve; the CLI prints them. Tests: the
   order group's V-Carve cases. [4]
10. **V-Carve goldens.** The eight PureCutCNC cases with notes; the
    in-order-per-region comparison mode. Tests: `golden::vcarve_*`.
    [5, 7]
11. **V-Carve docs.** V-Carve chapter (medial lines, flat depth, clearing
    passes, start depth, one pass, offset, bit angles), Warnings, Safety
    (S17), The project file (v20); quick start "A V-carved sign" written
    against the sign with plain hanging holes. [4–10]
12. **3.1 bench check.** The sign exported and carved without its
    countersinks, recorded in the functional spec as a partial cut.

Slices 1, 2 and 3 have no dependencies: three agents start at once.

### 3.2 Countersink (format v21)

Goal: the sign complete, the phase's first real cut.

13. **Format v21 and the countersink.** `DrillCycle::Countersink`,
    `v20_to_v21`, validation; the generator with the retract from the
    stock top and the engagement check; `start_depth_uncleared` for a
    countersink. Tests: the drill (countersink) group with its H3 and M2
    additions, the order group's countersink cases. [3.1] Until S18,
    `check` and `export` refuse a countersink (`CamError::NotSupportedYet`)
    and S9 holds its cuts to the fail-closed `Unresolved` (#425).
13a. **A V-Carve floor clears a start plane** (#426). The shared query
    becomes `dc_project::cleared_start_plane`, with the V-Carve flat-floor
    arm ("Start depth", step 2); `start_depth_mismatch` names the V-Carve.
    [13]
14. **S18.** `material.rs`. Tests: the safety S18 tests. [13]
15. **Countersink docs and the sign.** Drill chapter update, the quick
    start's countersink step, `fixtures/sign.svg` final,
    `cli::sign_exports_four_programs_in_tool_run_order`,
    `quick_start::v_carved_sign_chapter_runs`. [13, 14]
16. **3.2 bench check.** Test part 1 cut and recorded (functional spec,
    "Sign"). Exit: acceptance 48 to 55 and 61 green.

### 3.3 Inlay (formats v22, v23, v24 and v25)

Goal: the lettered inlay pair cut and fitted.

17. **Format v22 and the Inlay model.** `InlayOp`, `InlaySide` (gone in
    v25, slice 20e),
    `v21_to_v22`, validation with the male engagement sum. Tests: the
    dc-project Inlay tests. [3.2]
18. **Inlay geometry.** `dc-geometry::surround`, `Transform::mirror_x_about`.
    Tests: `inlay::surround_of_a_star_with_a_hole_has_the_hole_as_a_region`,
    `inlay::mirror_negates_bulges_and_keeps_areas`. [none]
19. **Inlay generator.** `dc-cam::inlay`: both sides from `Carve`, the
    mirror note, the plug-base warning, clearing on either side; S17 for
    the surround in `material.rs`. Tests: the inlay group, the safety
    inlay tests. [17, 18]
20. **Inlay docs and fixtures.** Inlay chapter (method, four values,
    two-file workflow, fit procedure, gluing and planing),
    `fixtures/star.svg`, `cli::star_female_and_male_export`. [19]
20a. **Male Inlay cut-out, format v23** (#443, added 2026-10-30 at the
    maintainer's request, before the bench check): `InlayOp::cut_out`,
    `v22_to_v23`, its validation, the cut-out run ("Algorithms → Inlay",
    step 6), S9 on the floor ring, the star male's fixture with a
    cut-out and the docs. Tests: the cut-out tests of the inlay,
    validate, migrate, serialize, safety and CLI groups,
    `quick_start::inlay_pair_chapter_runs`. [19, 20]
20b. **A lettered inlay example** (#446, added 2026-10-30 at the
    maintainer's request: the star is symmetric, so the tutorial never
    showed the male's mirror). The monogram "DC" in Droid Serif Bold,
    `fixtures/dc-inlay.svg` from `tools/parity/inlay-fixture.ts`, on a
    110 × 80 × 12 mm board, replaces the star in the fixtures, the quick
    start and the Inlay chapter; the unit tests keep the star as
    geometry. Tests: `cli::lettered_inlay_female_and_male_export`,
    `cli::lettered_inlay_fits_its_board`,
    `cli::lettered_male_is_the_mirror_of_the_female`,
    `quick_start::inlay_pair_chapter_runs`. [20a]
20c. **Male band with an open, rounded outer edge, format v24** (#448,
    added 2026-10-31 at the maintainer's request after previewing a
    lettered male): `InlayOp::outline_rounding_mm`, `v23_to_v24`,
    `inlay_outline_rounding`; the edge rule (`dc_project::male_band`), the
    rounded band, the open edge and the cut-out in the floor ring
    ("Algorithms → Inlay", steps 4 and 6; along its outer edge since
    #461), S17 against the
    open edge and S9 on the open floor; the lettered pair rounded by
    10 mm. A user-drawn boundary is deferred to #449. Tests: the #448
    group of the test plan, `cli::lettered_inlay_female_and_male_export`,
    `cli::lettered_inlay_fits_its_board`,
    `cli::v23_project_exports_the_same_program`,
    `quick_start::inlay_pair_chapter_runs`. [20b]
20d. **The male band clipped to the stock** (#495, maintainer decision
    2026-11-02, found on the maintainer's eagle male): the band and the
    cut-out clipped to the board, `inlay_band_clipped`,
    `InlayTooNearEdge` ("Algorithms → Inlay", steps 4 and 6), S17 and S9
    on the clipped band. No format change. Tests: the #495 group of the
    test plan. [20c]
20e. **One-file Inlay, format v25** (#498, maintainer decision
    2026-11-02, from the phase 4 UI review, #460: an inlay is one design
    cut into two boards from one set of numbers, and the user should not
    keep two project files in step by hand). `InlayOp::side` gives way to
    `InlayOp::male_board` (`MaleBoard`), `v24_to_v25` with its migration
    note, `Board` on runs and programs, `generate_inlay` for both boards,
    `inlay_male_board_unset`, the three male-board validation codes and
    the removal of `inlay_cut_out_needs_male`, the safety pass per board,
    the CLI's per-board names, grouping, `--board` and preview, the one
    tutorial project `tests/fixtures/dc.dcv` (the old pairs kept as
    migration fixtures in `crates/dreamcarve/tests/fixtures/inlay-migration/`).
    Photo V-Carve moves to format v26. Tests: the #498 group of the test
    plan. [20d]
21. **3.3 bench check.** Test part 2 cut, fitted and recorded. Exit:
    acceptance 56 and 57 green.

### 3.4 Photo V-Carve (format v26)

Goal: a photograph carved.

22. **Format v26: bitmaps and Photo V-Carve** (v24 until #448, v25
    until #498). `Bitmap`, `BitmapId`,
    `MAX_BITMAP_SIDE_PX`, `Bitmap::header` and `pixels` with their limits,
    `PhotoVCarveOp`, `PhotoStrategy`, `v25_to_v26`, validation; the `png`
    and `base64` dependencies. Tests: the dc-project bitmap and Photo
    tests with the M4 additions. [3.3]
23. **Bitmap import and the CLI.** `dc-import::bitmap` with `png` and
    `zune-jpeg`; `init` on a picture; `image add`/`list`; the preview
    image. Tests: the dc-import bitmap group, `cli::init_on_a_picture_*`,
    `cli::image_*`, `cli::moved_dcv_*`, `cli::preview_draws_*`. [22]
24. **Photo generator and S19.** `dc-cam::photo`,
    `SequenceInput::stay_down_links`, `MAX_PHOTO_MOVES`, S19 in
    `assert_safe` and `material.rs`, `start_depth_uncleared` for a Photo.
    Tests: the photo group, the safety S19 tests,
    `golden::phase_1_and_2_cases_are_unchanged`. [22]
25. **Photo goldens.** `tools/parity/photo/capture.py` and the two
    RasterCarve cases. Tests: `golden::photo_*`. [24]
26. **Photo docs.** Photo V-Carve chapter, Images chapter (`image`,
    pictures in the file), The project file (v26),
    `cli::photo_quick_start_runs`. [23, 24]
27. **3.4 bench check.** Test part 3 cut and recorded. Exit: acceptance
    58 to 60 green.

### 3.5 Release

28. **Docs sweep.** `CONTEXT.md` terms (Flat Depth, Start Depth, Clearing,
    Inlay, Glue Gap, Plug Depth, Photo V-Carve, Bitmap, Countersink), the
    Porting and Golden-corpus contributing chapters, Crate map, Tool
    library (V-bit angles, cone height). [all]
29. **Exit-criteria audit and tag.** The audit as phase 2's (#255), the
    changelog, `v0.3.0` with binaries.

## Security review (2026-10-28)

A security review of this draft found four High findings and five
Medium ones, all fixed above and marked by their numbers: **H1** the
sequencer plunged a chain to its deepest Z at its entry
(`CutBlock::entry_z_mm`); **H2** the stay-down link rule, taken from
upstream, let a link at depth run below the final V surface (links now
pass the cone test at their own depth, or retract); **H3** S17's depth
range refused every descent through the air above a start plane, and the
countersink's retract rapid landed below the stock top (the descent
exemption; the retract from the stock top); **H4** the cone-height check
counted only the carve's own depth, so a male Inlay with ordinary values
on a 1/4 in 90° bit would have run the shank into the plug (engagement
from the stock top); **M1** the sampled cone test could under-read by
half a sample (exact, as S9); **M2** an uncleared start depth exported a
cut the project could not see the material of (`start_depth_uncleared`
and `start_plane_cleared_externally`, decided by the maintainer); **M3**
the safety pass called a generator (`floor_regions` and `surround` moved
to `dc-geometry`); **M4** the stored PNG had no limits in `dc-project`
(`MAX_BITMAP_SIDE_PX`, header-first checks, decoder limits, checked
arithmetic); **M5** the hatch golden would have driven GPL code from a
script (dropped; capture hygiene stated). Low items L1–L6 and the
dependency notes are applied where they are cited. The review also
confirmed the clearing trust model (reach from the tool, never from a
toolpath; the form became an embedded list the same day without
changing that), the mirror and surround geometry,
the floor and offset rings, the countersink derivation, the 3D block
reading of S13, medial determinism, the migration and the CLI's staged
writes.

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
