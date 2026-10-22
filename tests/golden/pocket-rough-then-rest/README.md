# Golden case: pocket-rough-then-rest

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case pocket-rough-then-rest --post grblhal`.

- Source: `tools/parity/cases/pocket-rough-then-rest.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (pocket), `op2` (pocket)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`pocket_rough_then_rest`)
  compares the first operation's cutting moves at each Z level within
  0.01 mm, plus rounding, as `pocket-offset-rect` does. The second
  operation is checked, not compared: see below.
- DreamCarve project: `tests/golden/pocket-rough-then-rest/dreamcarve.dcv`.
  The `pocket-offset-rect` rectangle, 6 mm deep in 3 mm passes. `o1`
  roughs it with the 6.35 mm tool (T1) leaving `allowance_mm: 0.5`; `o2`
  is `rest_of: o1` with a 3.175 mm tool (T2, feed 1200, plunge 400,
  18000 RPM). One program with `T2 M6` (`tool_change_mode: m6`).
- PureCutCNC has no rest machining in this sense. Its equivalent is `op1`,
  a rough pocket with `stockToLeaveRadial: 0.5`, then `op2`, a finish
  pocket (`pass: finish`, walls only) with the 3.175 mm tool.
- Intentional difference (#163): DreamCarve's rest Pocket clears what the
  rough left at every one of the rough's levels. PureCutCNC's finish pass
  cuts the wall once, at the final depth, plunging the small tool 6 mm.
  So the second operations are not compared move for move. Instead the
  test checks that:
  - each program's cuts with both tools, at the final depth, sweep every
    point of the pocket the 3.175 mm tool can reach (sampled every
    0.25 mm);
  - DreamCarve's rest operation cuts at every level its rough did;
  - PureCutCNC's finish cuts at the final depth alone.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
