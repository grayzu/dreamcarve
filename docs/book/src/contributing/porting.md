# Porting from PureCutCNC

DreamCarve re-implements PureCutCNC's behaviour in Rust. Before designing
anything, look at how PureCutCNC does it.

## Finding the code

A local checkout is expected at `../purecutcnc`. Start from its `INDEX.md`,
which points to a per-folder `INDEX.md`. The engine lives under
`src/engine/`: `toolpaths/` for generators, `gcode/` for post-processing,
`simulation/` for the preview.

Each DreamCarve design spec has a "PureCutCNC sources" table mapping our
modules to theirs: `docs/specs/phase-1/design.md`,
`docs/specs/phase-2/design.md`, `docs/specs/phase-3/design.md` and
`docs/specs/phase-4/design.md`. A row whose PureCutCNC column says "none"
is DreamCarve's own design; see
[What is not a port](#what-is-not-a-port).

PureCutCNC's UI is React and CSS. What phase 4 ports from it is
behaviour, into `dc-app`, which has no egui: the editor's table of
fields (`editor/fields.rs`), the Tool Library's filter (`library.rs`),
hit testing and the camera (`viewport/`), the direction arrows. Its
components, its layout and its styles are not ported. The app's window,
its five steps (`dc_app::steps`) and its panels in `dreamcarve-app` are
DreamCarve's own design (`docs/specs/phase-4/redesign.md`).

## What changes in the port

- **Coordinates.** PureCutCNC works in a Y-down screen frame and flips once
  at G-code output. DreamCarve is Y-up throughout (ADR-0001). Every winding,
  inside/outside, and arc-direction rule is re-derived, not copied. Use the
  checklist below.
- **Idiom.** Port behaviour, not structure. A TypeScript union of string
  literals becomes an enum; an optional field with a sentinel becomes an
  `Option`; a thrown error becomes a typed `Result`. See
  `docs/rust-guidelines.md`.
- **Missing values.** When a feed, plunge rate, or spindle speed is
  missing, DreamCarve fails; it never substitutes a value the user did not
  set for that tool. Check every `||` and `??` fallback in upstream code
  whose result reaches the machine.
- **Controller data.** Post-processor definitions start from PureCutCNC's
  but are checked against each controller's own source and documentation.
  `crates/dc-project/src/definitions/README.md` records what changed and why.
- **Scope.** PureCutCNC's process rules (its GitHub board, fast lane, npm
  gates) apply to that repository only.

## The Y-up checklist

Go through this for every function you port that touches geometry.

1. **List the direction-sensitive rules** in the upstream code: the sign of
   a signed area, which way a contour winds, the sign of an offset for
   inside or outside, climb or conventional as clockwise or
   counter-clockwise, `G2` or `G3`, the sign of an arc's bulge or sweep,
   which side of a path a lead-in or tab sits on, and any sort or
   comparison on Y.
2. **Re-derive each one in Y-up**, from the geometry, not by negating the
   upstream rule. Write the derivation in the doc comment, as
   `dc_cam::traversal_is_ccw` does for climb milling.
3. **Use DreamCarve's conventions.** Outer contours are counter-clockwise
   and holes clockwise (`Contour`). A positive bulge is a counter-clockwise
   arc. Viewed from +Z with Y away from the operator.
4. **Flip only at the edges.** An importer of a Y-down format flips Y
   once (SVG: `y' = height_mm − y`); DXF is already Y-up and is not
   flipped. Renderers flip for the screen. Generators and
   post-processors never flip. A Y mirror in `dc-cam` or `dc-gcode` is a
   bug.
5. **Check edge and corner names.** A point at PureCutCNC `y` is at
   DreamCarve `height − y`, so "top" and "bottom" swap. Check every "top",
   "bottom", "min Y" and "max Y" in names and comments, and every origin
   choice. `crates/dc-gcode/tests/golden.rs` shows how the golden cases
   match PureCutCNC's origin to DreamCarve's.
6. **Port the tests too**, with the upstream header. Where Y-up changes an
   expected value, the test comment says what changed and why. A ported
   module's tests go where its others are: at its foot, or in its
   `tests.rs` ([Crate map](crate-map.md#modules)).
7. **Test the direction by name.** A test such as
   `climb_outside_is_clockwise` in `dc-cam`'s `profile/tests.rs` pins the
   re-derived rule, so a regression names itself.
8. **Compare against the golden corpus.** PureCutCNC's G-code is already in
   the machine frame, so a correct port matches it. If no case covers the
   behaviour, add one; see [Golden corpus](golden-corpus.md).

## Y-up rules settled in phase 2

Phase 2 re-derived these from PureCutCNC's Y-down code. Each is pinned by
a test named after the rule; reuse them rather than deriving again. The
design spec (`docs/specs/phase-2/design.md`) states each Y-up form with a
"Y-down note".

| Rule | DreamCarve (Y-up, seen from +Z) | PureCutCNC (Y-down) | Where |
| --- | --- | --- | --- |
| Profile lap direction | Climb runs outside a part clockwise, inside a hole counter-clockwise; conventional reverses both (phase 1). | Mirrored. | `dc_cam::traversal_is_ccw` (`profile.rs`) |
| Pocket wall and island rings | Climb runs the wall ring and every ring stepping in from it **counter-clockwise**, an island ring and every ring stepping out from it **clockwise**; conventional reverses both. | "Conventional is a no-op on Clipper output", because its positive-area outers are already machine-clockwise. In Y-up a positive-area (CCW) outer is climb, so conventional must reverse. | `dc_cam::ring_is_ccw` (`pocket.rs`); tests `climb_on_pocket_wall_is_counter_clockwise`, `climb_on_island_is_clockwise`, `conventional_reverses_both` |
| Helix sense | Climb inside a wall (pocket, inside Profile, hole) turns **counter-clockwise**, round a part (outside Profile) **clockwise**: the sense of the ring or lap it enters. | `helixAngularDirection` returns the negation of DreamCarve's sign. | `dc_cam::helix_is_ccw` (`entry.rs`); test `helix_sense_climb_inside_is_ccw` |
| Raster angle | `angle_deg` is counter-clockwise from +X as drawn; 0 to 180. | Rotates by `−pocketAngle` in its Y-down frame, so its +45° is DreamCarve's −45°, that is 135°. The `pocket-raster-45deg` golden `.camj` says `pocketAngle: -45` for DreamCarve's 45°. | `raster_lines` (`pocket/raster.rs`); [Golden corpus](golden-corpus.md#the-cases) |
| Raster scan start | The first line is half a stepover below the **top** of the tool-centre area (`max_y − s/2`), stepping down. | Scans from `minY + s/2`, which in Y-down is the top. Keeping upstream's expression in Y-up started at the bottom (#195). | `raster_lines`; test `scan_starts_half_a_step_below_the_top` |
| Lead side | The free side is found by probing the waste domain; a lead on the left of travel turns counter-clockwise about its centre, on the right clockwise. Nothing is copied from upstream's signs. | Y-down signs. | `lead.rs` module doc |
| Corner relief | Each cleared loop is walked with the cleared side on the left, its winding taken from its signed area, never assumed. A T-bone takes the longer edge; on a tie the more X-parallel, then the one pointing more to +X, then more to +Y, so a Y-down tie-break would pick the other edge. | Fixed `t_bone` (X-parallel) and `longest_edge` styles. | `t_bone_edge` (`relief.rs`) |
| Arc fitting | Fits in project space, before the machine transform (a translation and a signed axis map, never a mirror), so a negative cross sum about the centre is clockwise. | Fits machine coordinates after its Y flip. | `arcfit.rs` module doc |
| DXF import | **No flip.** DXF is Y-up like DreamCarve: `y' = y × scale`, and a bulge keeps its sign. Only the SVG importer flips. | Flips Y and arc direction to reach its Y-down frame. | `dxf.rs`; test `bulge_sign_is_not_flipped` |

Two conventions settled in phase 2 are not about Y but trip a port the
same way:

- **Fusion chamfer angle.** Autodesk's tool schema describes `TA` as the
  included angle, but Fusion's own 90° chamfer mills carry `TA` 45: it is
  the half angle. A chamfer mill imports as a V-bit with
  `included_angle_deg = 2 × TA` (functional spec, open question 10;
  `dc-library/src/fusion.rs`, test `chamfer_mill_ta_45_is_a_90_degree_v_bit`).
  Do not re-derive this from the schema text.
- **Drill pecks count from the retract height**, as a controller's `G83`
  and `G73` count from `R`, not from the stock top as upstream does (#153;
  `drill.rs`). The golden Drill cases carry the `PecksFromRetractPlane`
  difference for this.

## Y-up rules settled in phase 3

Phase 3 ported PureCutCNC's medial axis and its V-Carve generators. Most
of what changed hands is stated in the design spec
(`docs/specs/phase-3/design.md`, "Overview" and "PureCutCNC sources") and
in each module's doc comment, under "What changed in the port".

| Rule | DreamCarve (Y-up, seen from +Z) | PureCutCNC (Y-down) | Where |
| --- | --- | --- | --- |
| Medial axis | **Nothing to re-derive.** The Delaunay-dual skeleton of a region is the same under a reflection, so a clockwise outer gives the same nodes and edges as a counter-clockwise one. | The same construction, on Y-down loops. | `dc_geometry::medial`; test `winding_does_not_change_the_graph` (`medial/tests.rs`) |
| Medial links | A link between two chain ends stays down only when the straight move passes the exact cone test at its own depth; otherwise the bit retracts. Not about Y, but not ported. | Bridges chain ends tool-down within 0.9 of the disc the bit carves at the surface, which can run below the final V surface (security review H2). | `vcarve/medial.rs` module doc |
| V-Carve offset rings | Ring direction as a Pocket's (`ring_is_ccw`): climb runs an outer's rings counter-clockwise and an island's clockwise. Every level is inset from the region itself, not from the level before. | Each level inset from the one before, in its Y-down frame. | `vcarve/offset.rs`; tests `offset_ring_direction_matches_pocket_rings`, `offset_climb_and_conventional_reverse_every_ring` |
| Male Inlay mirror | The male is its targets mirrored in X about their bounding box's centre line, in place (`Transform::mirror_x_about`). The mirror reverses winding and negates bulges, and `Contour::new` makes an outer counter-clockwise again. | None: the Inlay is Corbin's Workshop's method, not a port. | `dc_cam::inlay::male_mirror`; test `male_mirrors_about_the_targets_centre_line` |
| Picture rows | A bitmap's top row is at its footprint's highest Y, as it shows on screen: row 0 is the top row, at the highest Y. | None (no Photo V-Carve). RasterCarve's origin is the picture's top-left corner, Y up, so its picture lies at negative Y. | `dc_cam::photo`; test `top_row_is_at_the_highest_y` |
| Photo groove family | One groove passes through the picture's top-left corner, RasterCarve's origin, so the goldens compare groove for groove. | None. | `dc_cam::photo`; test `one_groove_passes_through_the_top_left_corner` |

## The drawing tools (phase 5)

Phase 5 ports PureCutCNC's sketch layer, and takes some behaviour from a
second reference. `docs/specs/phase-5/design.md`, "Reference sources",
has the table of which DreamCarve module follows which upstream file.

**Ported from PureCutCNC**, with the ported-file header: the shape
constructions (rectangle, ellipse, polygon, slot, gear), the path edits
(split, delete, break, fillet, chamfer, the spline through points),
segment intersections, snapping's kinds and ranks, the drawing tools'
click sequences and typed fields, node editing, arrays, align and
distribute, Make Unique, flat folders, the tape measure, and the
single-line font *Simple Stroke*.

**Not ported**: the constraint solver and everything that calls it,
driving and named dimensions, dimension annotations, regions and feature
roles other than construction, text on arcs and paths, canned models,
the second face and rotary work. They are out of the phase's scope.

**Graphite** ([GraphiteEditor/Graphite](https://github.com/GraphiteEditor/Graphite),
Apache-2.0) is the second reference, for how a vector editor behaves
where PureCutCNC has nothing: press, drag and release as a second way
to give two points, `Shift` and `Alt` on the box tools, several nodes
selected and moved together, a corner node beside a smooth one, and
changing a segment between line, arc and curve. That behaviour was
re-implemented from a written survey
(`docs/research/graphite-tools-digest.md`), not transcribed, so those
files carry the DreamCarve header. A file that does transcribe a
Graphite file takes the ported-file header with `Copyright (c) Graphite
contributors` and Graphite's path and commit, and `NOTICE` gains
Graphite in the same change.

Two things are DreamCarve's own: the Text tool's layout, on the
`skrifa` crate and the bundled fonts ([Licensing](licensing.md#bundled-fonts)),
and trimming a piece from the middle of an outline, which upstream
leaves undone.

Every winding and direction was re-derived for Y-up: every drawn outline
is anticlockwise, an array's rows advance upwards, and Align's **Top**
is the greatest Y.

## What is not a port

Some of DreamCarve's features have no PureCutCNC code behind them. They
follow the public documentation of other products and are designed in
the spec, with tests derived from the geometry instead of goldens where
no program exists to compare against:

- **Clearing passes and the V-bit floor** follow VCarve's "Use Clearance
  Tools" and Corbin's Workshop's pocket setup. PureCutCNC clamps a carve's
  depth at `maxCarveDepth` and clears nothing.
- **Inlay** follows Corbin's Workshop's V-bit inlay method, which VCarve's
  "V-Carve Inlay" also follows, built from the V-Carve generator and the
  geometry crate.
- **Image prep** extends what PureCutCNC's SVG import reads (paint intent
  and the ambiguous-nesting checks of `svg.ts` and `classifier.ts`); the
  stroke outlines, fill resolution, speck filter and smoothing are
  DreamCarve's own.
- **The tapered ball nose** follows Fusion's tool library for its values
  and VCarve's and Corbin's documentation for its behaviour; its geometry
  is derived in the design spec, with analytic tests.
- **Photo V-Carve** follows VCarve's PhotoVCarve and RasterCarve's
  documented behaviour and command-line defaults. **RasterCarve is
  GPLv2+**, which is not compatible with Apache-2.0: never read its source
  to port or to fix a difference, and never copy from it. Only the G-code
  its own command line writes on DreamCarve's own fixture pictures enters
  the repository, as goldens ([Golden corpus](golden-corpus.md#photo-v-carve-cases)).

Files of these features carry the DreamCarve header, not the upstream one.
The same holds for any reference that is not Apache-2.0-compatible: it
may describe behaviour, never supply code ([Licensing](licensing.md)).

## Attribution

A file that ports or adapts PureCutCNC code keeps the upstream copyright
and names the source file; see [Licensing](licensing.md). Tests ported from
PureCutCNC do the same.
