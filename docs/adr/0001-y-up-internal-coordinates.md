---
status: accepted
date: 2026-10-14
---

# Internal geometry is Y-up, Z-up, millimetres, machine-native

PureCutCNC, our reference implementation, stores geometry in screen
coordinates (Y grows downward) and mirrors Y exactly once, inside the
post-processor, when writing G-code. DreamCarve stores geometry the way a
3-axis router sees it: X right, Y away from the operator, Z up, in
millimetres. The only flip is in the screen renderer.

We chose this because G-code export is the safety-critical boundary and
should be a translation, not a mirror. Mirroring the plane inverts every
direction-sensitive rule: winding (outer versus hole), inside versus outside
offsets, climb versus conventional, G2 versus G3, and lead-in tangents. We
would rather pay that cost once per rendered frame than once per emitted
program.

## Consequences

- Every algorithm ported from PureCutCNC must have its winding and arc
  direction assumptions re-derived, not copied. The golden corpus
  (`docs/plan.md`, "Parity corpus") catches mistakes, since PureCutCNC's
  exported G-code is already Y-up and compares directly.
- Closed outer profiles are counter-clockwise; holes are clockwise.
- Importers (SVG, DXF, fonts) flip Y at the import boundary. SVG and font
  coordinates are Y-down; DXF is Y-up.
