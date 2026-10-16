# Porting from PureCutCNC

DreamCarve re-implements PureCutCNC's behaviour in Rust. Before designing
anything, look at how PureCutCNC does it.

## Finding the code

A local checkout is expected at `../purecutcnc`. Start from its `INDEX.md`,
which points to a per-folder `INDEX.md`. The engine lives under
`src/engine/`: `toolpaths/` for generators, `gcode/` for post-processing,
`simulation/` for the preview.

Each DreamCarve design spec has a "PureCutCNC sources" table mapping our
modules to theirs. Phase 1's is in `docs/specs/phase-1/design.md`.

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
  `crates/dc-gcode/src/definitions/README.md` records what changed and why.
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
4. **Flip only at the edges.** Importers flip Y once (SVG:
   `y' = height_mm − y`); renderers flip for the screen. Generators and
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

## Attribution

A file that ports or adapts PureCutCNC code keeps the upstream copyright
and names the source file; see [Licensing](licensing.md). Tests ported from
PureCutCNC do the same.
