# Porting from PureCutCNC

DreamCarve re-implements PureCutCNC's behaviour in Rust. Before designing
anything, look at how PureCutCNC does it.

## Finding the code

A local checkout is expected at `../purecutcnc`. Start from its `INDEX.md`,
which points to a per-folder `INDEX.md`. The engine lives under
`src/engine/`: `toolpaths/` for generators, `gcode/` for post-processing,
`simulation/` for the preview.

Each DreamCarve design spec has a "PureCutCNC sources" table mapping our
modules to theirs: `docs/specs/phase-1/design.md` and `docs/specs/phase-2/design.md`.

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
   expected value, the test comment says what changed and why.
7. **Test the direction by name.** A test such as
   `climb_outside_is_clockwise` in `dc-cam`'s `profile.rs` pins the
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

## Attribution

A file that ports or adapts PureCutCNC code keeps the upstream copyright
and names the source file; see [Licensing](licensing.md). Tests ported from
PureCutCNC do the same.
