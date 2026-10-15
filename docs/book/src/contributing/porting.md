# Porting from PureCutCNC

DreamCarve re-implements PureCutCNC's behaviour in Rust. Before designing
anything, look at how PureCutCNC does it.

## Finding the code

A local checkout is expected at `../purecutcnc`. Start from its `INDEX.md`,
which points to a per-folder `INDEX.md`. The engine lives under
`src/engine/`: `toolpaths/` for generators, `gcode/` for post-processing,
`simulation/` for the preview.

Each DreamCarve design spec has a "PureCutCNC sources" table mapping our
modules to theirs.

## What changes in the port

- **Coordinates.** PureCutCNC works in a Y-down screen frame and flips once at
  G-code output. DreamCarve is Y-up throughout (ADR-0001). Every winding,
  inside/outside, and arc-direction rule is re-derived, not copied. The
  golden corpus catches mistakes: PureCutCNC's G-code is machine-frame, so a
  correct port matches it.
- **Idiom.** Port behaviour, not structure. A TypeScript union of string
  literals becomes an enum; an optional field with a sentinel becomes an
  `Option`; a thrown error becomes a typed `Result`. See
  `docs/rust-guidelines.md`.
- **Scope.** PureCutCNC's process rules (its GitHub board, fast lane, npm
  gates) apply to that repository only.

## Attribution

A file that ports or adapts PureCutCNC code keeps the upstream copyright and
names the source file; see [Licensing](licensing.md). Tests ported from
PureCutCNC do the same.

## The golden corpus

`tools/parity/capture.ts` runs PureCutCNC's own engine on a case and writes
its G-code and toolpath to `tests/golden/<case>/`, with a README recording
the PureCutCNC commit. `cargo test` compares DreamCarve's output to those
files. Node is needed only to capture new cases, never to run the tests.
